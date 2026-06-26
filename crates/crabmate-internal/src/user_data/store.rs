//! 本机用户数据读写 API（供 HTTP handler 与 CLI 共用）。

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::io::{
    ensure_tree, read_json_file, read_json_file_or_default, read_secret_line, write_json_atomic,
    write_secret_line,
};
use super::mcp_slug::assign_slugs_from_names;
use super::path::{
    global_sessions_path, normalize_workspace_partition_path, user_data_root,
    workspace_manifest_path, workspace_partition_hash, workspace_sessions_path,
};
use super::types::{
    LlmOverridesFile, McpServerEntry, McpServersFile, McpServersFilePublic,
    McpServersImportResponse, SCHEMA_VERSION, SecretSlotStatus, SecretsStatusResponse,
    UserDataMeta, UserPrefs, WebSessionsFile, WorkspaceListEntry, WorkspaceManifest,
};

fn root() -> PathBuf {
    user_data_root()
}

fn meta_path(root: &Path) -> PathBuf {
    root.join("meta.json")
}

fn prefs_path(root: &Path) -> PathBuf {
    root.join("prefs.json")
}

fn llm_path(root: &Path) -> PathBuf {
    root.join("llm_overrides.json")
}

fn mcp_servers_path(root: &Path) -> PathBuf {
    root.join("mcp_servers.json")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 新建 MCP 服务器条目的稳定 id（`mcp_<毫秒>`）。
pub fn new_mcp_server_id() -> String {
    format!("mcp_{}", now_ms())
}

fn secret_path(root: &Path, name: &str) -> PathBuf {
    root.join("secrets").join(name)
}

fn sessions_path_for_workspace(root: &Path, effective_workspace: &str) -> PathBuf {
    match workspace_partition_hash(effective_workspace) {
        Some(h) => workspace_sessions_path(root, &h),
        None => global_sessions_path(root),
    }
}

/// 确保目录树存在（幂等）。
pub fn ensure_user_data_tree() -> Result<(), String> {
    ensure_tree(&root())
}

pub fn load_meta() -> UserDataMeta {
    let r = root();
    read_json_file_or_default(&meta_path(&r))
}

pub fn load_prefs() -> UserPrefs {
    read_json_file_or_default(&prefs_path(&root()))
}

pub fn save_prefs(prefs: &UserPrefs) -> Result<(), String> {
    let r = root();
    ensure_tree(&r)?;
    write_json_atomic(&prefs_path(&r), prefs)
}

pub fn load_llm_overrides() -> LlmOverridesFile {
    read_json_file_or_default(&llm_path(&root()))
}

pub fn save_llm_overrides(file: &LlmOverridesFile) -> Result<(), String> {
    let r = root();
    ensure_tree(&r)?;
    write_json_atomic(&llm_path(&r), file)
}

pub fn load_mcp_servers() -> McpServersFile {
    read_json_file_or_default(&mcp_servers_path(&root()))
}

pub fn save_mcp_servers(file: &McpServersFile) -> Result<(), String> {
    let r = root();
    ensure_tree(&r)?;
    write_json_atomic(&mcp_servers_path(&r), file)
}

/// PUT 时保留磁盘上已有 `command`（Web 不往返启动命令）。
pub fn merge_mcp_commands_from_stored(mut incoming: McpServersFile) -> McpServersFile {
    let existing = load_mcp_servers();
    let by_id: std::collections::HashMap<&str, &McpServerEntry> = existing
        .servers
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();
    for srv in &mut incoming.servers {
        if srv.command.trim().is_empty()
            && let Some(old) = by_id.get(srv.id.as_str())
        {
            srv.command = old.command.clone();
        }
    }
    incoming
}

/// 解析 MCP JSON 并追加到已存配置（含 `command`），落盘后返回完整文件。
pub fn append_mcp_json_import(value: &Value) -> Result<McpServersImportResponse, String> {
    let imported = super::mcp_json_import::import_mcp_json_value(value)?;
    let imported_count = imported.entries.len();
    let warnings = imported.warnings;
    let skipped_remote = imported.skipped_remote;
    let mut file = load_mcp_servers();
    file.servers.extend(imported.entries);
    let file = normalize_mcp_servers_file(file)?;
    save_mcp_servers(&file)?;
    Ok(McpServersImportResponse {
        file: McpServersFilePublic::from(&file),
        imported_count,
        warnings,
        skipped_remote,
    })
}

/// 校验并规范化 PUT 体：补 id/时间戳、从 `name` 重算 `slug`。
pub fn normalize_mcp_servers_file(mut file: McpServersFile) -> Result<McpServersFile, String> {
    file.schema_version = SCHEMA_VERSION;
    if file.tool_timeout_secs == 0 {
        file.tool_timeout_secs = 60;
    }
    let now = now_ms();
    for srv in &mut file.servers {
        if srv.id.trim().is_empty() {
            srv.id = new_mcp_server_id();
            srv.created_at_ms = now;
        }
        srv.id = srv.id.trim().to_string();
        srv.name = srv.name.trim().to_string();
        if srv.name.is_empty() {
            return Err("MCP 服务器 name 不能为空".to_string());
        }
        srv.command = srv.command.trim().to_string();
        if srv.enabled && srv.command.is_empty() {
            return Err(format!("已启用的 MCP 服务器「{}」须填写 command", srv.name));
        }
        if srv.created_at_ms == 0 {
            srv.created_at_ms = now;
        }
        srv.updated_at_ms = now;
    }
    assign_slugs_from_names(&mut file.servers);
    Ok(file)
}

fn legacy_mcp_display_name(command: &str) -> String {
    let token = command.split_whitespace().next().unwrap_or("mcp");
    let base = std::path::Path::new(token)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(token);
    if base.is_empty() {
        "Legacy MCP".to_string()
    } else {
        format!("Legacy {base}")
    }
}

/// 若 user-data 尚无 MCP 配置且 TOML 启用了单条 `mcp_command`，一次性导入并落盘。
pub fn maybe_import_legacy_toml_mcp(
    mcp_enabled: bool,
    mcp_command: &str,
    mcp_tool_timeout_secs: u64,
) -> Result<bool, String> {
    let mut file = load_mcp_servers();
    if !file.servers.is_empty() {
        return Ok(false);
    }
    let cmd = mcp_command.trim();
    if !mcp_enabled || cmd.is_empty() {
        return Ok(false);
    }
    let now = now_ms();
    file.global_enabled = true;
    file.tool_timeout_secs = mcp_tool_timeout_secs.max(1);
    file.servers.push(McpServerEntry {
        id: new_mcp_server_id(),
        name: legacy_mcp_display_name(cmd),
        slug: String::new(),
        command: cmd.to_string(),
        enabled: true,
        created_at_ms: now,
        updated_at_ms: now,
    });
    let file = normalize_mcp_servers_file(file)?;
    save_mcp_servers(&file)?;
    Ok(true)
}

/// 读取 MCP 配置；必要时从 TOML 一次性导入 legacy 单服务器。
pub fn load_mcp_servers_with_legacy_import(
    mcp_enabled: bool,
    mcp_command: &str,
    mcp_tool_timeout_secs: u64,
) -> McpServersFile {
    let _ = maybe_import_legacy_toml_mcp(mcp_enabled, mcp_command, mcp_tool_timeout_secs);
    load_mcp_servers()
}

pub fn load_web_sessions(effective_workspace: &str) -> WebSessionsFile {
    read_json_file_or_default(&sessions_path_for_workspace(&root(), effective_workspace))
}

pub fn save_web_sessions(effective_workspace: &str, file: &WebSessionsFile) -> Result<(), String> {
    let r = root();
    ensure_tree(&r)?;
    if let Some(h) = workspace_partition_hash(effective_workspace) {
        let norm = normalize_workspace_partition_path(effective_workspace);
        if !norm.is_empty() {
            let manifest = WorkspaceManifest {
                workspace_root: effective_workspace.trim().to_string(),
                normalized: norm,
            };
            write_json_atomic(&workspace_manifest_path(&r, &h), &manifest)?;
        }
    }
    write_json_atomic(&sessions_path_for_workspace(&r, effective_workspace), file)
}

pub fn list_workspaces() -> Result<Vec<WorkspaceListEntry>, String> {
    let r = root();
    let ws_root = r.join("workspaces");
    if !ws_root.is_dir() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&ws_root).map_err(|e| format!("列举工作区桶: {e}"))? {
        let entry = entry.map_err(|e| format!("列举工作区桶: {e}"))?;
        let hash = entry.file_name().to_string_lossy().to_string();
        let manifest_path = workspace_manifest_path(&r, &hash);
        let root_display = if manifest_path.is_file() {
            read_json_file::<WorkspaceManifest>(&manifest_path)
                .map(|m| m.workspace_root)
                .unwrap_or_else(|_| format!("(hash {hash})"))
        } else {
            format!("(hash {hash})")
        };
        out.push(WorkspaceListEntry {
            hash,
            workspace_root: root_display,
        });
    }
    out.sort_by(|a, b| a.workspace_root.cmp(&b.workspace_root));
    Ok(out)
}

pub fn write_secret_client_llm(api_key: &str) -> Result<(), String> {
    let p = secret_path(&root(), "client_llm");
    if api_key.trim().is_empty() {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    write_secret_line(&p, api_key)
}

pub fn write_secret_executor_llm(api_key: &str) -> Result<(), String> {
    let p = secret_path(&root(), "executor_llm");
    if api_key.trim().is_empty() {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    write_secret_line(&p, api_key)
}

pub fn write_secret_web_api_bearer(token: &str) -> Result<(), String> {
    let p = secret_path(&root(), "web_api_bearer");
    if token.trim().is_empty() {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    write_secret_line(&p, token)
}

fn slot_status(path: &Path) -> SecretSlotStatus {
    match read_secret_line(path) {
        Some(s) => {
            let suffix = if s.len() >= 4 {
                Some(s[s.len().saturating_sub(4)..].to_string())
            } else {
                Some("****".to_string())
            };
            SecretSlotStatus { set: true, suffix }
        }
        None => SecretSlotStatus::default(),
    }
}

pub fn secrets_status() -> SecretsStatusResponse {
    let r = root();
    SecretsStatusResponse {
        client_llm: slot_status(&secret_path(&r, "client_llm")),
        executor_llm: slot_status(&secret_path(&r, "executor_llm")),
        web_api_bearer: slot_status(&secret_path(&r, "web_api_bearer")),
    }
}

/// 供 `POST /chat` 合并：仅返回密钥明文（勿记录日志）。
pub fn read_secret_client_llm() -> Option<String> {
    read_secret_line(&secret_path(&root(), "client_llm"))
}

pub fn read_secret_executor_llm() -> Option<String> {
    read_secret_line(&secret_path(&root(), "executor_llm"))
}

/// `web_sessions.json` 的 `sessions` 须为 JSON 数组。
pub fn validate_sessions_value(sessions: &Value) -> Result<(), String> {
    if sessions.is_array() {
        Ok(())
    } else {
        Err("sessions 须为 JSON 数组".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_root() -> PathBuf {
        static SLOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
        let slot = SLOT.get_or_init(|| Mutex::new(None));
        let mut g = slot.lock().unwrap();
        if g.is_none() {
            let dir = std::env::temp_dir()
                .join(format!("crabmate-user-data-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            // SAFETY: 测试进程内独占临时目录，无并发读写该环境变量。
            unsafe {
                std::env::set_var("CM_CRABMATE_USER_DATA_DIR", dir.display().to_string());
            }
            *g = Some(dir);
        }
        g.clone().unwrap()
    }

    #[test]
    fn prefs_roundtrip() {
        let _root = test_root();
        let p = UserPrefs {
            locale: Some("zh-Hans".to_string()),
            ..UserPrefs::default()
        };
        save_prefs(&p).expect("save");
        let loaded = load_prefs();
        assert_eq!(loaded.locale.as_deref(), Some("zh-Hans"));
    }

    #[test]
    fn normalize_assigns_slug_from_name() {
        let _root = test_root();
        use crate::user_data::SCHEMA_VERSION;
        use crate::user_data::types::{McpServerEntry, McpServersFile};
        let file = normalize_mcp_servers_file(McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers: vec![McpServerEntry {
                id: "mcp_test".into(),
                name: "My Server".into(),
                slug: String::new(),
                command: "echo mcp".into(),
                enabled: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            }],
        })
        .expect("normalize");
        assert_eq!(file.servers[0].slug, "my_server");
    }
}
