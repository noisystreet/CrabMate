//! 本机用户数据读写 API（供 HTTP handler 与 CLI 共用）。

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::io::{ensure_tree, read_json_file, read_json_file_or_default, write_json_atomic};
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
    let path = llm_path(&root());
    let mut file: LlmOverridesFile = read_json_file_or_default(&path);
    let before = file.saved_models.clone();
    if let Err(error) =
        super::saved_model_secrets::prepare_saved_model_secrets(&mut file.saved_models)
    {
        super::saved_model_secrets::scrub_saved_model_api_keys(&mut file.saved_models);
        tracing::warn!(
            target: "crabmate",
            error = %error,
            "迁移已保存模型 API 密钥到系统钥匙串失败；旧文件暂不改写"
        );
        return file;
    }
    if before != file.saved_models
        && let Err(error) = write_json_atomic(&path, &file)
    {
        tracing::warn!(
            target: "crabmate",
            error = %error,
            "清理 llm_overrides 中的旧 API 密钥字段失败"
        );
    }
    file
}

pub fn save_llm_overrides(file: &LlmOverridesFile) -> Result<(), String> {
    let r = root();
    ensure_tree(&r)?;
    let path = llm_path(&r);
    let mut old: LlmOverridesFile = read_json_file_or_default(&path);
    super::saved_model_secrets::prepare_saved_model_secrets(&mut old.saved_models)?;
    let old_accounts = super::saved_model_secrets::saved_model_accounts(&old.saved_models);
    let mut sanitized = file.clone();
    super::saved_model_secrets::prepare_saved_model_secrets(&mut sanitized.saved_models)?;
    let new_accounts = super::saved_model_secrets::saved_model_accounts(&sanitized.saved_models);
    write_json_atomic(&path, &sanitized)?;
    super::saved_model_secrets::delete_removed_saved_model_secrets(&old_accounts, &new_accounts)
}

pub fn load_mcp_servers() -> McpServersFile {
    read_json_file_or_default(&mcp_servers_path(&root()))
}

pub fn save_mcp_servers(file: &McpServersFile) -> Result<(), String> {
    let r = root();
    ensure_tree(&r)?;
    let old_ids: std::collections::HashSet<String> = load_mcp_servers()
        .servers
        .into_iter()
        .map(|server| server.id)
        .collect();
    let ids: Vec<String> = file.servers.iter().map(|s| s.id.clone()).collect();
    for id in &ids {
        let _ = read_secret_mcp_bearer(id);
    }
    write_json_atomic(&mcp_servers_path(&r), file)?;
    let keep: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    for removed_id in old_ids.iter().filter(|id| !keep.contains(id.as_str())) {
        write_secret_mcp_bearer(removed_id, "")?;
    }
    prune_mcp_bearer_secrets(&ids)?;
    Ok(())
}

/// PUT 时保留磁盘上已有启动规格（Web 不往返 `command`/`args`/`env`/`cwd`/`url`/`headers`）。
pub fn merge_mcp_commands_from_stored(mut incoming: McpServersFile) -> McpServersFile {
    let existing = load_mcp_servers();
    let by_id: std::collections::HashMap<&str, &McpServerEntry> = existing
        .servers
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();
    for srv in &mut incoming.servers {
        let Some(old) = by_id.get(srv.id.as_str()) else {
            continue;
        };
        if srv.command.trim().is_empty() {
            srv.command = old.command.clone();
        }
        if srv.args.is_empty() {
            srv.args = old.args.clone();
        }
        if srv.env.is_empty() {
            srv.env = old.env.clone();
        }
        if srv
            .cwd
            .as_ref()
            .map(|c| c.trim().is_empty())
            .unwrap_or(true)
        {
            srv.cwd = old.cwd.clone();
        }
        if srv
            .url
            .as_ref()
            .map(|u| u.trim().is_empty())
            .unwrap_or(true)
        {
            srv.url = old.url.clone();
        }
        if srv.headers.is_empty() {
            srv.headers = old.headers.clone();
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
    // Web/JSON 导入已进入 user-data，关闭 TOML/`CM_MCP_COMMAND` 一次性窗口。
    file.toml_legacy_imported = true;
    let file = normalize_mcp_servers_file(file)?;
    save_mcp_servers(&file)?;
    Ok(McpServersImportResponse {
        file: mcp_servers_file_public(&file),
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
        srv.args = srv.args.iter().map(|a| a.trim().to_string()).collect();
        srv.env = srv
            .env
            .iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| (k.trim().to_string(), v.clone()))
            .collect();
        srv.cwd = srv
            .cwd
            .take()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        srv.url = srv
            .url
            .take()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        srv.headers = srv
            .headers
            .iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| (k.trim().to_string(), v.clone()))
            .collect();
        let has_cmd = srv.has_stdio();
        let has_url = srv.has_remote_url();
        if has_cmd && has_url {
            return Err(format!(
                "MCP 服务器「{}」不能同时填写 command 与 url",
                srv.name
            ));
        }
        if srv.enabled && !has_cmd && !has_url {
            return Err(format!(
                "已启用的 MCP 服务器「{}」须填写 command 或 url",
                srv.name
            ));
        }
        if has_url {
            crabmate_mcp::resolve::validate_mcp_remote_url(srv.url.as_deref().unwrap_or(""))?;
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
///
/// 导入成功或已有非空 `servers` 时写入 [`McpServersFile::toml_legacy_imported`]，之后不再重导。
pub fn maybe_import_legacy_toml_mcp(
    mcp_enabled: bool,
    mcp_command: &str,
    mcp_tool_timeout_secs: u64,
) -> Result<bool, String> {
    let mut file = load_mcp_servers();
    if file.toml_legacy_imported {
        return Ok(false);
    }
    if !file.servers.is_empty() {
        // 存量列表：视为已越过一次性导入窗口，落标记以免清空后再次导入。
        file.toml_legacy_imported = true;
        save_mcp_servers(&file)?;
        return Ok(false);
    }
    let cmd = mcp_command.trim();
    if !mcp_enabled || cmd.is_empty() {
        return Ok(false);
    }
    let now = now_ms();
    file.global_enabled = true;
    file.tool_timeout_secs = mcp_tool_timeout_secs.max(1);
    file.toml_legacy_imported = true;
    file.servers.push(McpServerEntry {
        id: new_mcp_server_id(),
        name: legacy_mcp_display_name(cmd),
        slug: String::new(),
        command: cmd.to_string(),
        args: Vec::new(),
        env: std::collections::BTreeMap::new(),
        cwd: None,
        url: None,
        headers: std::collections::BTreeMap::new(),
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
    super::credential_store::write_migrating_secret(
        "client_llm",
        &secret_path(&root(), "client_llm"),
        api_key,
    )
}

pub fn write_secret_executor_llm(api_key: &str) -> Result<(), String> {
    super::credential_store::write_migrating_secret(
        "executor_llm",
        &secret_path(&root(), "executor_llm"),
        api_key,
    )
}

pub fn write_secret_web_api_bearer(token: &str) -> Result<(), String> {
    super::credential_store::write_migrating_secret(
        "web_api_bearer",
        &secret_path(&root(), "web_api_bearer"),
        token,
    )
}

pub fn read_secret_web_api_bearer() -> Option<String> {
    super::credential_store::read_migrating_secret(
        "web_api_bearer",
        &secret_path(&root(), "web_api_bearer"),
    )
}

/// 校验 MCP server id 是否可安全用作 secret 文件名片段。
pub fn validate_mcp_secret_server_id(server_id: &str) -> Result<&str, String> {
    let id = server_id.trim();
    if id.is_empty() || id.len() > 128 {
        return Err("MCP 服务器 id 无效".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("MCP 服务器 id 含非法字符，无法写入 Bearer secret".to_string());
    }
    Ok(id)
}

fn mcp_bearer_secret_name(server_id: &str) -> Result<String, String> {
    let id = validate_mcp_secret_server_id(server_id)?;
    Ok(format!("mcp_bearer_{id}"))
}

/// 远程 MCP Bearer：系统钥匙串账户 `mcp_bearer_{id}`。
pub fn write_secret_mcp_bearer(server_id: &str, token: &str) -> Result<(), String> {
    let name = mcp_bearer_secret_name(server_id)?;
    super::credential_store::write_migrating_secret(&name, &secret_path(&root(), &name), token)
}

pub fn read_secret_mcp_bearer(server_id: &str) -> Option<String> {
    let name = mcp_bearer_secret_name(server_id).ok()?;
    super::credential_store::read_migrating_secret(&name, &secret_path(&root(), &name))
}

pub fn mcp_bearer_is_set(server_id: &str) -> bool {
    read_secret_mcp_bearer(server_id).is_some_and(|s| !s.trim().is_empty())
}

/// 删除已不存在于配置中的 `mcp_bearer_*` orphan secrets。
pub fn prune_mcp_bearer_secrets(keep_ids: &[String]) -> Result<(), String> {
    let secrets_dir = root().join("secrets");
    if !secrets_dir.is_dir() {
        return Ok(());
    }
    let keep: std::collections::HashSet<&str> = keep_ids.iter().map(String::as_str).collect();
    for entry in std::fs::read_dir(&secrets_dir).map_err(|e| format!("列举 secrets: {e}"))? {
        let entry = entry.map_err(|e| format!("列举 secrets: {e}"))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = name.strip_prefix("mcp_bearer_") else {
            continue;
        };
        if !keep.contains(id) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// 公开 MCP 配置体（含各 server `has_bearer`）。
pub fn mcp_servers_file_public(file: &McpServersFile) -> McpServersFilePublic {
    McpServersFilePublic::from_file_with_bearer(file, mcp_bearer_is_set)
}

fn slot_status_from_secret(secret: Option<String>) -> SecretSlotStatus {
    match secret {
        Some(s) => {
            let suffix = if s.chars().count() >= 4 {
                let tail = s
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                Some(tail)
            } else {
                Some("****".to_string())
            };
            SecretSlotStatus { set: true, suffix }
        }
        None => SecretSlotStatus::default(),
    }
}

pub fn secrets_status() -> SecretsStatusResponse {
    SecretsStatusResponse {
        client_llm: slot_status_from_secret(read_secret_client_llm()),
        executor_llm: slot_status_from_secret(read_secret_executor_llm()),
        web_api_bearer: slot_status_from_secret(read_secret_web_api_bearer()),
    }
}

/// 供 `POST /chat` 合并：仅返回密钥明文（勿记录日志）。
pub fn read_secret_client_llm() -> Option<String> {
    super::credential_store::read_migrating_secret(
        "client_llm",
        &secret_path(&root(), "client_llm"),
    )
}

pub fn read_secret_executor_llm() -> Option<String> {
    super::credential_store::read_migrating_secret(
        "executor_llm",
        &secret_path(&root(), "executor_llm"),
    )
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
    use std::sync::{Mutex, MutexGuard, OnceLock};

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

    /// 共享 `CM_CRABMATE_USER_DATA_DIR` 下的 `mcp_servers.json` 不可并行写。
    fn lock_mcp_servers_tests() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 测试用命名钥匙串（`credential_store` 进程内 HashMap）不可并行读写。
    fn lock_named_secret_tests() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn prefs_roundtrip() {
        let _root = test_root();
        let p = UserPrefs {
            locale: Some("zh-Hans".to_string()),
            recent_workspace_roots: vec!["/tmp/a".into(), "/tmp/b".into()],
            last_workspace_root: Some("/tmp/a".into()),
            ..UserPrefs::default()
        };
        save_prefs(&p).expect("save");
        let loaded = load_prefs();
        assert_eq!(loaded.locale.as_deref(), Some("zh-Hans"));
        assert_eq!(
            loaded.recent_workspace_roots,
            vec!["/tmp/a".to_string(), "/tmp/b".to_string()]
        );
        assert_eq!(loaded.last_workspace_root.as_deref(), Some("/tmp/a"));
    }

    #[test]
    fn llm_overrides_migrates_saved_model_key_out_of_json() {
        let root = test_root();
        std::fs::create_dir_all(&root).expect("create user data root");
        let path = llm_path(&root);
        std::fs::write(
            &path,
            serde_json::json!({
                "saved_models": [{
                    "api_base": "https://store-test.invalid/v1",
                    "model": "model-store-migration",
                    "api_key": "example-token"
                }]
            })
            .to_string(),
        )
        .expect("write old overrides");

        let loaded = load_llm_overrides();

        let saved = loaded.saved_models[0]
            .as_object()
            .expect("saved model object");
        assert!(!saved.contains_key("api_key"));
        assert_eq!(saved.get("has_api_key"), Some(&Value::Bool(true)));
        let persisted: Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read overrides"))
                .expect("parse overrides");
        assert!(
            persisted["saved_models"][0].get("api_key").is_none(),
            "persisted llm_overrides must not contain API keys"
        );
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
                args: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                url: None,
                headers: std::collections::BTreeMap::new(),
                enabled: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            }],
            toml_legacy_imported: false,
        })
        .expect("normalize");
        assert_eq!(file.servers[0].slug, "my_server");
    }

    #[test]
    fn merge_preserves_structured_launch_fields() {
        let _root = test_root();
        let _mcp_lock = lock_mcp_servers_tests();
        use crate::user_data::SCHEMA_VERSION;
        use crate::user_data::types::{McpServerEntry, McpServersFile};
        use std::collections::BTreeMap;
        let mut env = BTreeMap::new();
        env.insert("RUST_LOG".into(), "warn".into());
        let stored = McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers: vec![McpServerEntry {
                id: "mcp_x".into(),
                name: "X".into(),
                slug: "x".into(),
                command: "fanalyzer".into(),
                args: vec!["mcp".into(), "serve".into()],
                env,
                cwd: Some("/tmp/ws".into()),
                url: None,
                headers: BTreeMap::new(),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            }],
            toml_legacy_imported: false,
        };
        save_mcp_servers(&stored).expect("save");
        let incoming = McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 90,
            servers: vec![McpServerEntry {
                id: "mcp_x".into(),
                name: "X Renamed".into(),
                slug: String::new(),
                command: String::new(),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                headers: std::collections::BTreeMap::new(),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            }],
            toml_legacy_imported: false,
        };
        let merged = merge_mcp_commands_from_stored(incoming);
        assert_eq!(merged.servers[0].command, "fanalyzer");
        assert_eq!(merged.servers[0].args, vec!["mcp", "serve"]);
        assert_eq!(
            merged.servers[0].env.get("RUST_LOG").map(String::as_str),
            Some("warn")
        );
        assert_eq!(merged.servers[0].cwd.as_deref(), Some("/tmp/ws"));
    }

    #[test]
    fn mcp_bearer_secret_roundtrip_and_public_flag() {
        let _root = test_root();
        let _secrets = lock_named_secret_tests();
        write_secret_mcp_bearer("mcp_remote1", "tok-secret").expect("write");
        assert!(mcp_bearer_is_set("mcp_remote1"));
        assert_eq!(
            read_secret_mcp_bearer("mcp_remote1").as_deref(),
            Some("tok-secret")
        );
        let file = McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers: vec![McpServerEntry {
                id: "mcp_remote1".into(),
                name: "R".into(),
                slug: "r".into(),
                command: String::new(),
                args: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                url: Some("https://example.com/mcp".into()),
                headers: std::collections::BTreeMap::new(),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            }],
            toml_legacy_imported: false,
        };
        let pub_file = mcp_servers_file_public(&file);
        assert!(pub_file.servers[0].has_bearer);
        assert!(pub_file.servers[0].has_url);
        write_secret_mcp_bearer("mcp_remote1", "").expect("clear");
        assert!(!mcp_bearer_is_set("mcp_remote1"));
    }

    #[test]
    fn remaining_secret_files_migrate_to_keyring() {
        let root = test_root();
        let _secrets_lock = lock_named_secret_tests();
        let secrets = root.join("secrets");
        std::fs::create_dir_all(&secrets).expect("create secrets dir");
        let web_legacy = secrets.join("web_api_bearer");
        let mcp_legacy = secrets.join("mcp_bearer_mcp_migration_test");
        std::fs::write(&web_legacy, "web-example-token").expect("write web legacy");
        std::fs::write(&mcp_legacy, "mcp-example-token").expect("write mcp legacy");

        assert_eq!(
            read_secret_web_api_bearer().as_deref(),
            Some("web-example-token")
        );
        assert_eq!(
            read_secret_mcp_bearer("mcp_migration_test").as_deref(),
            Some("mcp-example-token")
        );
        assert!(!web_legacy.exists());
        assert!(!mcp_legacy.exists());

        write_secret_web_api_bearer("").expect("clear web");
        write_secret_mcp_bearer("mcp_migration_test", "").expect("clear mcp");
    }

    #[test]
    fn save_mcp_servers_clears_removed_server_bearer() {
        let _root = test_root();
        let _mcp_lock = lock_mcp_servers_tests();
        let _secrets = lock_named_secret_tests();
        write_secret_mcp_bearer("mcp_to_remove", "tok-remove-me").expect("write");
        assert!(mcp_bearer_is_set("mcp_to_remove"));
        let with_server = McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers: vec![McpServerEntry {
                id: "mcp_to_remove".into(),
                name: "Temp".into(),
                slug: "temp".into(),
                command: String::new(),
                args: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                url: Some("https://example.com/mcp".into()),
                headers: std::collections::BTreeMap::new(),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            }],
            toml_legacy_imported: false,
        };
        save_mcp_servers(&with_server).expect("save with server");
        let kept = McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers: vec![McpServerEntry {
                id: "mcp_kept".into(),
                name: "Kept".into(),
                slug: "kept".into(),
                command: "true".into(),
                args: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                url: None,
                headers: std::collections::BTreeMap::new(),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            }],
            toml_legacy_imported: false,
        };
        save_mcp_servers(&kept).expect("save without removed id");
        assert!(!mcp_bearer_is_set("mcp_to_remove"));
    }

    #[test]
    fn legacy_toml_mcp_imports_once_and_sets_marker() {
        let _root = test_root();
        let _mcp_lock = lock_mcp_servers_tests();
        save_mcp_servers(&McpServersFile::default()).expect("reset");

        assert!(
            maybe_import_legacy_toml_mcp(true, "echo mcp-legacy", 60).expect("import"),
            "first import should write a server"
        );
        let after = load_mcp_servers();
        assert!(after.toml_legacy_imported);
        assert_eq!(after.servers.len(), 1);
        assert!(after.servers[0].command.contains("echo"));

        assert!(
            !maybe_import_legacy_toml_mcp(true, "echo other", 60).expect("second"),
            "marker blocks re-import"
        );
        assert_eq!(load_mcp_servers().servers.len(), 1);
    }

    #[test]
    fn legacy_toml_mcp_marker_blocks_reimport_after_clear() {
        let _root = test_root();
        let _mcp_lock = lock_mcp_servers_tests();
        save_mcp_servers(&McpServersFile::default()).expect("reset");
        assert!(maybe_import_legacy_toml_mcp(true, "echo once", 60).expect("import"));

        let mut cleared = load_mcp_servers();
        cleared.servers.clear();
        // 保留 toml_legacy_imported=true
        save_mcp_servers(&cleared).expect("clear servers");

        assert!(!maybe_import_legacy_toml_mcp(true, "echo again", 60).expect("no reimport"));
        assert!(load_mcp_servers().servers.is_empty());
        assert!(load_mcp_servers().toml_legacy_imported);
    }

    #[test]
    fn existing_servers_set_legacy_marker_without_reimport() {
        let _root = test_root();
        let _mcp_lock = lock_mcp_servers_tests();
        use crate::user_data::SCHEMA_VERSION;
        let existing = McpServersFile {
            schema_version: SCHEMA_VERSION,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers: vec![McpServerEntry {
                id: "mcp_existing".into(),
                name: "Existing".into(),
                slug: "existing".into(),
                command: "true".into(),
                args: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                url: None,
                headers: std::collections::BTreeMap::new(),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            }],
            toml_legacy_imported: false,
        };
        save_mcp_servers(&existing).expect("save existing");

        assert!(!maybe_import_legacy_toml_mcp(true, "echo should-not-import", 60).expect("skip"));
        let after = load_mcp_servers();
        assert!(after.toml_legacy_imported);
        assert_eq!(after.servers.len(), 1);
        assert_eq!(after.servers[0].command, "true");
    }
}
