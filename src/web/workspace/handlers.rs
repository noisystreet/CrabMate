use std::path::Path;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use log::error;
use serde_json;

use crate::AppState;
use crate::text_encoding::{decode_bytes_strict, parse_text_encoding_name};
use crate::web::http_types::validation::{
    clamp_workspace_search_max_results, validate_workspace_file_write_request,
    validate_workspace_query_encoding_optional, workspace_search_pattern_or_error,
};
use crate::web::http_types::workspace::{
    WorkspaceEntry, WorkspaceFileDeleteResponse, WorkspaceFileQuery, WorkspaceFileReadResponse,
    WorkspaceFileWriteBody, WorkspaceFileWriteResponse, WorkspacePickResponse,
    WorkspaceProfileResponse, WorkspaceQuery, WorkspaceResponse, WorkspaceSearchBody,
    WorkspaceSearchResponse, WorkspaceSetBody,
};
#[cfg(unix)]
use crate::workspace::fs::{
    open_directory_under_root, open_existing_file_under_root, open_file_write_under_root,
    unlink_file_under_root,
};
use crate::workspace::path::{
    WorkspacePathError, resolve_web_workspace_read_path, resolve_web_workspace_write_path,
    validate_effective_workspace_base, validate_workspace_set_path,
};
#[cfg(unix)]
use libc;
#[cfg(unix)]
use nix::dir::Type;
#[cfg(unix)]
use nix::fcntl::AtFlags;
#[cfg(unix)]
use nix::sys::stat::fstatat;

const WORKSPACE_FILE_READ_MAX_BYTES: u64 = 1_048_576;

async fn workspace_file_read_resolve(
    state: &Arc<AppState>,
    query: &WorkspaceFileQuery,
) -> Result<
    (
        std::path::PathBuf,
        std::path::PathBuf,
        crate::text_encoding::TextEncodingName,
    ),
    Json<WorkspaceFileReadResponse>,
> {
    if let Err(e) = validate_workspace_query_encoding_optional(query.encoding.as_deref()) {
        return Err(Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some(e),
        }));
    }
    let base_canonical = match effective_workspace_base_canonical(state).await {
        Ok(p) => p,
        Err(e) => {
            return Err(Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(e.user_message()),
            }));
        }
    };
    let path = query.path.trim();
    if path.is_empty() {
        return Err(Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some("path 不能为空".to_string()),
        }));
    }
    let canonical =
        match resolve_web_workspace_read_path(&base_canonical, Some(query.path.as_str())) {
            Ok(p) => p,
            Err(e) => {
                return Err(Json(WorkspaceFileReadResponse {
                    content: String::new(),
                    error: Some(e.user_message()),
                }));
            }
        };
    let enc_name = match parse_text_encoding_name(query.encoding.as_deref()) {
        Ok(n) => n,
        Err(msg) => {
            return Err(Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(msg),
            }));
        }
    };
    Ok((base_canonical, canonical, enc_name))
}

/// 解析当前会话工作区根为 canonical 路径，并校验仍在 `workspace_allowed_roots` 内、非敏感目录。
async fn effective_workspace_base_canonical(
    state: &Arc<AppState>,
) -> Result<std::path::PathBuf, WorkspacePathError> {
    let base_str = state.effective_workspace_path().await;
    if base_str.trim().is_empty() {
        return Err(WorkspacePathError::WebEffectiveWorkspaceUnset);
    }
    let base = Path::new(&base_str);
    let base_canonical = base
        .canonicalize()
        .map_err(WorkspacePathError::WorkspaceResolveFailed)?;
    let cfg = state.http.cfg.read().await;
    validate_effective_workspace_base(&cfg, &base_canonical)?;
    Ok(base_canonical)
}

/// 工作区「选目录」占位接口（历史兼容）：已不再绑定原生对话框依赖，始终返回 `path: null`。
/// Web 侧请在输入框中填写路径后按 Enter 提交 [`workspace_set_handler`]。
pub async fn workspace_pick_handler() -> Json<WorkspacePickResponse> {
    Json(WorkspacePickResponse { path: None })
}

/// 设置当前工作区根目录（来自前端）。非空路径须已存在、为目录，且落在配置的 `workspace_allowed_roots` 内（未配置时仅允许 `run_command_working_dir` 及其子目录），并且不得命中敏感系统目录黑名单。
pub async fn workspace_set_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceSetBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let raw = body.path.as_deref().map(|s| s.trim()).unwrap_or("");
    let mut guard = state.http.workspace_override.write().await;
    // None 表示“从未设置过”；Some("") 表示“显式选择默认目录”；Some("...") 表示指定路径（存规范绝对路径）
    if raw.is_empty() {
        *guard = Some(String::new());
        return Ok(Json(serde_json::json!({ "ok": true, "path": "" })));
    }
    let cfg = state.http.cfg.read().await;
    let canon = match validate_workspace_set_path(&cfg, raw) {
        Ok(p) => p,
        Err(e) => {
            let status = if e.is_policy_denied() {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_REQUEST
            };
            return Err((
                status,
                Json(serde_json::json!({ "ok": false, "error": e.user_message() })),
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
    let base_canonical = match effective_workspace_base_canonical(&state).await {
        Ok(p) => p,
        Err(WorkspacePathError::WebEffectiveWorkspaceUnset) => {
            return Json(WorkspaceResponse {
                path: String::new(),
                entries: Vec::new(),
                error: None,
            });
        }
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "workspace list base error kind={} msg={}",
                e.kind(),
                e
            );
            return Json(WorkspaceResponse {
                path: String::new(),
                entries: Vec::new(),
                error: Some(e.user_message()),
            });
        }
    };
    let canonical = match resolve_web_workspace_read_path(&base_canonical, query.path.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceResponse {
                path: base_canonical.display().to_string(),
                entries: Vec::new(),
                error: Some(e.user_message()),
            });
        }
    };
    let path_str = canonical.display().to_string();

    #[cfg(unix)]
    {
        let base = base_canonical.clone();
        let can = canonical.clone();
        let path_for_resp = path_str.clone();
        match tokio::task::spawn_blocking(move || {
            let (mut dir, _) = open_directory_under_root(&base, &can)
                .map_err(|e| format!("无法读取工作目录: {e}"))?;
            let mut names: Vec<String> = Vec::new();
            let mut types_hint: Vec<Option<Type>> = Vec::new();
            for ent in dir.iter() {
                let ent = ent.map_err(|e| format!("读取目录项失败: {e}"))?;
                let name_c = ent.file_name();
                let nb = name_c.to_bytes();
                if nb == b"." || nb == b".." {
                    continue;
                }
                names.push(String::from_utf8_lossy(nb).to_string());
                types_hint.push(ent.file_type());
            }
            let mut entries = Vec::new();
            for (name, hint) in names.into_iter().zip(types_hint.into_iter()) {
                let is_dir = match hint {
                    Some(Type::Directory) => true,
                    Some(Type::Symlink) | None => {
                        let st = fstatat(&dir, name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                            .map_err(|e| format!("读取目录项失败: {e}"))?;
                        (st.st_mode & libc::S_IFMT) == libc::S_IFDIR
                    }
                    _ => false,
                };
                entries.push(WorkspaceEntry { name, is_dir });
            }
            entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
            Ok::<_, String>((path_for_resp, entries))
        })
        .await
        {
            Ok(Ok((p, entries))) => Json(WorkspaceResponse {
                path: p,
                entries,
                error: None,
            }),
            Ok(Err(msg)) => {
                log::warn!("{}", msg);
                Json(WorkspaceResponse {
                    path: path_str,
                    entries: Vec::new(),
                    error: Some(msg),
                })
            }
            Err(e) => {
                log::warn!("workspace list join error: {}", e);
                Json(WorkspaceResponse {
                    path: path_str,
                    entries: Vec::new(),
                    error: Some("列出工作区失败".to_string()),
                })
            }
        }
    }

    #[cfg(not(unix))]
    {
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
        entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
        Json(WorkspaceResponse {
            path: path_str,
            entries,
            error: None,
        })
    }
}

/// 在当前工作区内搜索文件内容（基于 search_in_files/grep 工具），返回纯文本结果
pub async fn workspace_search_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceSearchBody>,
) -> Json<WorkspaceSearchResponse> {
    let pattern = match workspace_search_pattern_or_error(&body) {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceSearchResponse {
                output: String::new(),
                error: Some(e),
            });
        }
    };
    let base_canonical = match effective_workspace_base_canonical(&state).await {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceSearchResponse {
                output: String::new(),
                error: Some(e.user_message()),
            });
        }
    };
    let rel_path = match body.path.as_deref() {
        None => None,
        Some(raw) => {
            if raw.trim().is_empty() {
                None
            } else {
                match resolve_web_workspace_read_path(&base_canonical, Some(raw)) {
                    Ok(canonical) => match canonical.strip_prefix(&base_canonical) {
                        Ok(r) => Some(r.to_string_lossy().to_string()),
                        Err(_) => None,
                    },
                    Err(e) => {
                        return Json(WorkspaceSearchResponse {
                            output: String::new(),
                            error: Some(e.user_message()),
                        });
                    }
                }
            }
        }
    };
    let mut args = serde_json::json!({ "pattern": pattern });
    if let Some(p) = rel_path {
        args["path"] = serde_json::Value::String(p);
    }
    if let Some(m) = clamp_workspace_search_max_results(body.max_results) {
        args["max_results"] = serde_json::json!(m);
    }
    if let Some(ci) = body.case_insensitive {
        args["case_insensitive"] = serde_json::json!(ci);
    }
    if let Some(ih) = body.ignore_hidden {
        args["ignore_hidden"] = serde_json::json!(ih);
    }
    let args_json = args.to_string();
    let cfg_snap = {
        let g = state.http.cfg.read().await;
        g.clone()
    };
    let cfg_arc = Arc::new(cfg_snap);
    let work_dir = base_canonical.clone();
    let output = match tokio::task::spawn_blocking(move || {
        let ctx = crate::tools::tool_context_for(
            cfg_arc.as_ref(),
            cfg_arc.command_exec.allowed_commands.as_ref(),
            &work_dir,
        );
        crate::tools::run_tool("search_in_files", &args_json, &ctx)
    })
    .await
    {
        Ok(output) => output,
        Err(e) => {
            error!(
                target: "crabmate",
                "workspace_search 阻塞任务异常 error={}",
                e
            );
            return Json(WorkspaceSearchResponse {
                output: String::new(),
                error: Some("搜索执行失败，请稍后重试".to_string()),
            });
        }
    };
    Json(WorkspaceSearchResponse {
        output,
        error: None,
    })
}

/// 工作区文件读取：按 path 返回文件内容（path 为工作区内文件路径）
pub async fn workspace_file_read_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileReadResponse> {
    let (base_canonical, canonical, enc_name) =
        match workspace_file_read_resolve(&state, &query).await {
            Ok(x) => x,
            Err(e) => return e,
        };

    #[cfg(unix)]
    {
        use std::io::Read;
        let base = base_canonical.clone();
        let can = canonical.clone();
        let max_b = WORKSPACE_FILE_READ_MAX_BYTES;
        match tokio::task::spawn_blocking(move || -> Result<(String, _), String> {
            let opened = open_existing_file_under_root(&base, &can)
                .map_err(|e| format!("无法读取文件信息: {e}"))?;
            if opened.metadata.is_dir() {
                return Err("路径是目录，无法读取为文件".to_string());
            }
            let len = opened.metadata.len();
            if len > max_b {
                return Err(format!(
                    "文件过大（{} 字节），当前最多读取 {} 字节",
                    len, max_b
                ));
            }
            let mut f = opened.file;
            let mut raw = Vec::new();
            f.read_to_end(&mut raw)
                .map_err(|e| format!("读取文件失败: {e}"))?;
            decode_bytes_strict(&raw, enc_name)
        })
        .await
        {
            Ok(Ok((content, _))) => Json(WorkspaceFileReadResponse {
                content,
                error: None,
            }),
            Ok(Err(msg)) => Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(msg),
            }),
            Err(e) => Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(format!("读取文件任务失败: {}", e)),
            }),
        }
    }

    #[cfg(not(unix))]
    {
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
        let raw = match tokio::fs::read(&canonical).await {
            Ok(b) => b,
            Err(e) => {
                return Json(WorkspaceFileReadResponse {
                    content: String::new(),
                    error: Some(format!("读取文件失败: {}", e)),
                });
            }
        };
        match decode_bytes_strict(&raw, enc_name) {
            Ok((content, _)) => Json(WorkspaceFileReadResponse {
                content,
                error: None,
            }),
            Err(msg) => Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(msg),
            }),
        }
    }
}

/// 删除工作区内的文件：path 为工作区内文件路径，不能删除目录
pub async fn workspace_file_delete_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileDeleteResponse> {
    let base_canonical = match effective_workspace_base_canonical(&state).await {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileDeleteResponse {
                error: Some(e.user_message()),
            });
        }
    };
    if let Err(e) = validate_workspace_query_encoding_optional(query.encoding.as_deref()) {
        return Json(WorkspaceFileDeleteResponse { error: Some(e) });
    }
    let path = query.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileDeleteResponse {
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical =
        match resolve_web_workspace_read_path(&base_canonical, Some(query.path.as_str())) {
            Ok(p) => p,
            Err(e) => {
                return Json(WorkspaceFileDeleteResponse {
                    error: Some(e.user_message()),
                });
            }
        };

    #[cfg(unix)]
    {
        let base = base_canonical.clone();
        let can = canonical.clone();
        match tokio::task::spawn_blocking(move || {
            let opened = open_existing_file_under_root(&base, &can)
                .map_err(|e| format!("无法读取文件信息: {e}"))?;
            if opened.metadata.is_dir() {
                return Err("不支持删除目录".to_string());
            }
            unlink_file_under_root(&base, &can).map_err(|e| format!("删除文件失败: {e}"))
        })
        .await
        {
            Ok(Ok(())) => Json(WorkspaceFileDeleteResponse { error: None }),
            Ok(Err(msg)) => Json(WorkspaceFileDeleteResponse { error: Some(msg) }),
            Err(e) => Json(WorkspaceFileDeleteResponse {
                error: Some(format!("删除文件任务失败: {}", e)),
            }),
        }
    }

    #[cfg(not(unix))]
    {
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
}

#[cfg(unix)]
fn workspace_file_write_sync_unix(
    base: std::path::PathBuf,
    normalized: std::path::PathBuf,
    content: String,
    create_only: bool,
    update_only: bool,
) -> Result<(), String> {
    use std::io::{ErrorKind, Write};
    if let Some(parent) = normalized.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let mut f = match open_file_write_under_root(&base, &normalized, create_only, update_only) {
        Ok(f) => f,
        Err(e) if create_only && e.kind() == ErrorKind::AlreadyExists => {
            return Err("文件已存在，无法仅创建".to_string());
        }
        Err(e) if update_only && e.kind() == ErrorKind::NotFound => {
            return Err("文件不存在，无法仅修改".to_string());
        }
        Err(e) => {
            return Err(format!("打开文件失败: {e}"));
        }
    };
    f.write_all(content.as_bytes())
        .map_err(|e| format!("写入文件失败: {e}"))
}

/// 工作区文件写入：支持创建、写入（创建或覆盖）、仅创建、仅修改
pub async fn workspace_file_write_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceFileWriteBody>,
) -> Json<WorkspaceFileWriteResponse> {
    let base_canonical = match effective_workspace_base_canonical(&state).await {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileWriteResponse {
                error: Some(e.user_message()),
            });
        }
    };
    if let Err(e) = validate_workspace_file_write_request(&body) {
        return Json(WorkspaceFileWriteResponse { error: Some(e) });
    }
    let path = body.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileWriteResponse {
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_web_workspace_write_path(&base_canonical, path) {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileWriteResponse {
                error: Some(e.user_message()),
            });
        }
    };

    #[cfg(unix)]
    {
        let base = base_canonical.clone();
        let normalized = canonical.clone();
        let content = body.content;
        let create_only = body.create_only;
        let update_only = body.update_only;
        match tokio::task::spawn_blocking(move || {
            workspace_file_write_sync_unix(base, normalized, content, create_only, update_only)
        })
        .await
        {
            Ok(Ok(())) => Json(WorkspaceFileWriteResponse { error: None }),
            Ok(Err(msg)) => Json(WorkspaceFileWriteResponse { error: Some(msg) }),
            Err(e) => Json(WorkspaceFileWriteResponse {
                error: Some(format!("写入文件任务失败: {}", e)),
            }),
        }
    }

    #[cfg(not(unix))]
    {
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
}

/// 返回当前工作区的项目画像（Markdown）。与 `project_profile_inject_max_chars` 上限一致；为 0 时返回空正文。
pub async fn workspace_profile_handler(
    State(state): State<Arc<AppState>>,
) -> Json<WorkspaceProfileResponse> {
    let base_canonical = match effective_workspace_base_canonical(&state).await {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceProfileResponse {
                markdown: String::new(),
                error: Some(e.user_message()),
            });
        }
    };
    let max_chars = state
        .http
        .cfg
        .read()
        .await
        .context_bootstrap_inject
        .project_profile_inject_max_chars;
    let md_result = tokio::task::spawn_blocking(move || {
        crate::context_bootstrap::project_profile::build_project_profile_markdown(
            &base_canonical,
            max_chars,
        )
    })
    .await;
    match md_result {
        Ok(markdown) => Json(WorkspaceProfileResponse {
            markdown,
            error: None,
        }),
        Err(e) => Json(WorkspaceProfileResponse {
            markdown: String::new(),
            error: Some(format!("生成项目画像任务失败: {}", e)),
        }),
    }
}
