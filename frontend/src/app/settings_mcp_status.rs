//! MCP 设置页：探测状态合并与刷新。

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::user_data::{McpServerStatusEntryDto, McpServersFileDto, McpServersStatusDto};
use crate::i18n::Locale;

/// 设置页 MCP 区块共享信号（避免子组件形参过多）。
#[derive(Clone, Copy)]
pub(crate) struct McpSettingsSignals {
    pub locale: RwSignal<Locale>,
    pub file: ReadSignal<McpServersFileDto>,
    pub set_file: WriteSignal<McpServersFileDto>,
    pub baseline: RwSignal<McpServersFileDto>,
    pub status: ReadSignal<Option<McpServersStatusDto>>,
    pub set_status: WriteSignal<Option<McpServersStatusDto>>,
    pub busy: ReadSignal<bool>,
    pub set_busy: WriteSignal<bool>,
    pub probing: ReadSignal<bool>,
    pub set_probing: WriteSignal<bool>,
}

/// 设置页级 MCP 草稿（上提至 `SettingsPageView`，避免切量子页时丢失未保存删除）。
#[derive(Clone, Copy)]
pub(crate) struct McpSettingsPageState {
    pub file: ReadSignal<McpServersFileDto>,
    pub set_file: WriteSignal<McpServersFileDto>,
    pub baseline: RwSignal<McpServersFileDto>,
    pub import_json: RwSignal<String>,
    pub status: ReadSignal<Option<McpServersStatusDto>>,
    pub set_status: WriteSignal<Option<McpServersStatusDto>>,
    pub busy: ReadSignal<bool>,
    pub set_busy: WriteSignal<bool>,
    pub probing: ReadSignal<bool>,
    pub set_probing: WriteSignal<bool>,
    pub feedback: ReadSignal<Option<String>>,
    pub set_feedback: WriteSignal<Option<String>>,
}

impl McpSettingsPageState {
    #[must_use]
    pub(crate) fn new() -> Self {
        let (file, set_file) = signal(McpServersFileDto::default());
        let baseline = RwSignal::new(McpServersFileDto::default());
        let import_json = RwSignal::new(String::new());
        let (status, set_status) = signal(None::<McpServersStatusDto>);
        let (busy, set_busy) = signal(false);
        let (probing, set_probing) = signal(false);
        let (feedback, set_feedback) = signal(None::<String>);
        Self {
            file,
            set_file,
            baseline,
            import_json,
            status,
            set_status,
            busy,
            set_busy,
            probing,
            set_probing,
            feedback,
            set_feedback,
        }
    }

    #[must_use]
    pub(crate) fn is_dirty_tracked(&self) -> bool {
        mcp_draft_is_dirty(
            &self.file.get(),
            &self.baseline.get(),
            &self.import_json.get(),
        )
    }

    #[must_use]
    pub(crate) fn is_dirty_untracked(&self) -> bool {
        mcp_draft_is_dirty(
            &self.file.get_untracked(),
            &self.baseline.get_untracked(),
            &self.import_json.get_untracked(),
        )
    }

    pub(crate) fn discard_to_baseline(&self) {
        self.set_file.set(self.baseline.get_untracked());
        self.import_json.set(String::new());
        self.set_feedback.set(None);
    }

    #[must_use]
    pub(crate) fn as_row_ctx(&self, locale: RwSignal<Locale>) -> McpSettingsSignals {
        McpSettingsSignals {
            locale,
            file: self.file,
            set_file: self.set_file,
            baseline: self.baseline,
            status: self.status,
            set_status: self.set_status,
            busy: self.busy,
            set_busy: self.set_busy,
            probing: self.probing,
            set_probing: self.set_probing,
        }
    }
}

/// 草稿相对 baseline / 待合并 JSON 是否未保存。
#[must_use]
pub(crate) fn mcp_draft_is_dirty(
    file: &McpServersFileDto,
    baseline: &McpServersFileDto,
    pending_import: &str,
) -> bool {
    file != baseline || !pending_import.trim().is_empty()
}

pub(crate) fn merge_probe_into_status(
    base: &mut McpServersStatusDto,
    probes: &[McpServerStatusEntryDto],
) {
    for upd in probes {
        if let Some(row) = base.servers.iter_mut().find(|r| r.id == upd.id) {
            *row = upd.clone();
        } else {
            base.servers.push(upd.clone());
        }
    }
}

fn should_auto_probe_mcp(file: &McpServersFileDto) -> bool {
    file.global_enabled
        && file
            .servers
            .iter()
            .any(|s| s.enabled && (s.has_command || s.has_url))
}

fn validate_mcp_draft_for_save(file: &McpServersFileDto, loc: Locale) -> Result<(), String> {
    for s in &file.servers {
        if s.enabled && !s.has_command && !s.has_url {
            return Err(crate::i18n::settings_mcp_enabled_missing_command(
                loc, &s.name,
            ));
        }
    }
    Ok(())
}

pub(crate) async fn refresh_mcp_status_with_probe(
    loc: Locale,
    file: &McpServersFileDto,
) -> Option<McpServersStatusDto> {
    let mut base = crate::api::user_data::fetch_mcp_servers_status(loc)
        .await
        .unwrap_or_default();
    base.global_enabled = file.global_enabled;
    base.tool_timeout_secs = file.tool_timeout_secs;
    if should_auto_probe_mcp(file) {
        if let Ok(probes) = crate::api::user_data::post_mcp_servers_probe_all(loc).await {
            merge_probe_into_status(&mut base, &probes);
        }
    }
    Some(base)
}

pub(crate) async fn probe_all_and_merge(
    loc: Locale,
    file: &McpServersFileDto,
    status: Option<McpServersStatusDto>,
) -> Option<McpServersStatusDto> {
    let mut base = status.unwrap_or_default();
    base.global_enabled = file.global_enabled;
    base.tool_timeout_secs = file.tool_timeout_secs;
    if let Ok(list) = crate::api::user_data::post_mcp_servers_probe_all(loc).await {
        merge_probe_into_status(&mut base, &list);
    }
    Some(base)
}

pub(crate) async fn probe_one_and_merge(
    loc: Locale,
    server_id: &str,
    mut status: McpServersStatusDto,
) -> McpServersStatusDto {
    if let Ok(upd) = crate::api::user_data::post_mcp_server_probe(server_id, loc).await {
        merge_probe_into_status(&mut status, std::slice::from_ref(&upd));
    }
    status
}

pub(crate) fn spawn_probe_mcp_server(ctx: McpSettingsSignals, server_id: String) {
    let McpSettingsSignals {
        locale,
        status,
        set_status,
        set_busy,
        set_probing,
        ..
    } = ctx;
    set_busy.set(true);
    set_probing.set(true);
    spawn_local(async move {
        if let Some(st) = status.get_untracked() {
            let merged = probe_one_and_merge(locale.get_untracked(), &server_id, st).await;
            set_status.set(Some(merged));
        }
        set_busy.set(false);
        set_probing.set(false);
    });
}

/// 保存 MCP 配置（含可选 MCP JSON 合并）的异步任务参数。
pub(crate) struct McpSaveJob {
    pub loc: Locale,
    pub pending_import: String,
    pub import_json: RwSignal<String>,
    pub ctx: McpSettingsSignals,
    pub set_feedback: WriteSignal<Option<String>>,
}

pub(crate) fn spawn_reload_mcp(loc: Locale, state: McpSettingsPageState) {
    let McpSettingsPageState {
        set_file,
        baseline,
        set_status,
        set_probing,
        ..
    } = state;
    spawn_local(async move {
        set_probing.set(true);
        if let Ok(f) = crate::api::user_data::fetch_mcp_servers(loc).await {
            set_file.set(f.clone());
            baseline.set(f.clone());
            if let Some(st) = refresh_mcp_status_with_probe(loc, &f).await {
                set_status.set(Some(st));
            }
        }
        set_probing.set(false);
    });
}

pub(crate) fn spawn_save_mcp(job: McpSaveJob) {
    let McpSaveJob {
        loc,
        pending_import,
        import_json,
        ctx,
        set_feedback,
    } = job;
    let McpSettingsSignals {
        file,
        set_file,
        baseline,
        set_status,
        set_busy,
        set_probing,
        ..
    } = ctx;
    set_busy.set(true);
    set_probing.set(true);
    spawn_local(async move {
        if !pending_import.trim().is_empty() {
            match crate::api::user_data::post_mcp_servers_import(&pending_import, loc).await {
                Ok(outcome) => {
                    import_json.set(String::new());
                    set_file.set(outcome.file.clone());
                    baseline.set(outcome.file);
                    let mut msg = crate::i18n::settings_mcp_import_merged_on_save(loc);
                    if !outcome.skipped_remote.is_empty() {
                        msg.push('\n');
                        msg.push_str(&crate::i18n::settings_mcp_import_skipped_remote(
                            loc,
                            &outcome.skipped_remote.join(", "),
                        ));
                    }
                    for w in &outcome.warnings {
                        msg.push('\n');
                        msg.push_str(w);
                    }
                    set_feedback.set(Some(msg));
                }
                Err(e) => {
                    set_feedback.set(Some(e));
                    set_busy.set(false);
                    set_probing.set(false);
                    return;
                }
            }
        }
        let draft = file.get_untracked();
        if let Err(e) = validate_mcp_draft_for_save(&draft, loc) {
            set_feedback.set(Some(e));
            set_busy.set(false);
            set_probing.set(false);
            return;
        }
        match crate::api::user_data::put_mcp_servers(&draft, loc).await {
            Ok(()) => {
                set_feedback.set(Some(crate::i18n::settings_mcp_save(loc).to_string() + " ✓"));
                if let Ok(f) = crate::api::user_data::fetch_mcp_servers(loc).await {
                    set_file.set(f.clone());
                    baseline.set(f.clone());
                    if let Some(st) = refresh_mcp_status_with_probe(loc, &f).await {
                        set_status.set(Some(st));
                    }
                } else {
                    baseline.set(draft);
                }
            }
            Err(e) => set_feedback.set(Some(e)),
        }
        set_busy.set(false);
        set_probing.set(false);
    });
}

pub(crate) fn spawn_probe_all_mcp(ctx: McpSettingsSignals) {
    let McpSettingsSignals {
        locale,
        file,
        status,
        set_status,
        set_busy,
        set_probing,
        ..
    } = ctx;
    set_busy.set(true);
    set_probing.set(true);
    spawn_local(async move {
        let draft = file.get_untracked();
        let st = probe_all_and_merge(locale.get_untracked(), &draft, status.get_untracked()).await;
        set_status.set(st);
        set_busy.set(false);
        set_probing.set(false);
    });
}

#[cfg(test)]
mod mcp_draft_dirty_tests {
    use super::mcp_draft_is_dirty;
    use crate::api::user_data::{McpServerEntryDto, McpServersFileDto};

    fn sample_file(servers: Vec<McpServerEntryDto>) -> McpServersFileDto {
        McpServersFileDto {
            schema_version: 1,
            global_enabled: true,
            tool_timeout_secs: 60,
            servers,
        }
    }

    #[test]
    fn delete_server_marks_dirty() {
        let baseline = sample_file(vec![McpServerEntryDto {
            id: "mcp_a".into(),
            name: "A".into(),
            ..Default::default()
        }]);
        let draft = sample_file(vec![]);
        assert!(mcp_draft_is_dirty(&draft, &baseline, ""));
        assert!(!mcp_draft_is_dirty(&baseline, &baseline, ""));
    }

    #[test]
    fn pending_import_marks_dirty() {
        let file = sample_file(vec![]);
        assert!(mcp_draft_is_dirty(&file, &file, "{\"mcpServers\":{}}"));
    }
}
