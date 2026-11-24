//! 新会话 Token 基线的按角色计算与并行化（#1504）。
//!
//! 与 `health_status` 的分工：`GET /status` 与 serve 启动预热都只要「每个角色一份基线数字」，
//! 而每份都要独立装配 system 并做全量 BPE 编码、彼此无依赖 —— 计算放这里，
//! `health_status` 只负责缓存失效与 HTTP 形状。

use std::collections::BTreeMap;
use std::sync::Arc;

/// 单个角色的新会话基线 Token 数（`None` = 该角色 system 装配失败）。
///
/// 抽成独立函数是为了让 [`tiktoken_new_session_baselines_by_role`] 按角色并行：每份都要
/// 完整装配自己的 system（各角色的 L0–L6 / AGENTS.md / skills 索引不同），彼此**无共享
/// 可变状态**，因此可安全放进独立线程。
pub(super) fn tiktoken_baseline_tokens_for_role(
    cfg: &crate::AgentConfig,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
    workspace_root: &std::path::Path,
    memory_snippet: Option<String>,
    role: Option<&str>,
) -> Option<u32> {
    let system =
        crate::context_bootstrap::conversation_turn_bootstrap::augmented_system_for_new_conversation(
            cfg,
            role,
            tool_recorder,
            Some(workspace_root),
        )
        .ok()?;
    let messages =
        crate::context_bootstrap::conversation_turn_bootstrap::new_session_prompt_baseline_messages(
            cfg,
            &system,
            workspace_root,
            memory_snippet,
        );
    crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session(
        cfg, &messages,
    )
    .map(|snap| snap.prompt_tokens)
}

/// 各角色的新会话基线 Token 数（`""` 键是「无角色」那一档）。
///
/// 各项互相独立 → 按可用核数分批并行：每项都是 `system` 装配 + 全量 BPE 编码的纯 CPU 工作。
/// `memory_snippet` 只在并行段**之前**读一次盘；线程 panic 与装配失败都按「该项不产出」处理，
/// 与串行实现（`Err(_) => return`）的缺项语义一致。
pub(super) fn tiktoken_new_session_baselines_by_role(
    cfg: &crate::AgentConfig,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
    workspace_root: &std::path::Path,
    agent_role_ids: &[String],
) -> BTreeMap<String, u32> {
    // 备忘片段是磁盘内容，与角色无关：并行段之前取一次，避免每份都读盘。
    let memory_snippet = if cfg.context_bootstrap_inject.agent_memory_file_enabled {
        crate::memory::agent_memory::load_memory_snippet(
            workspace_root,
            cfg.context_bootstrap_inject.agent_memory_file.as_str(),
            cfg.context_bootstrap_inject.agent_memory_file_max_chars,
        )
    } else {
        None
    };
    let mut jobs: Vec<(String, Option<&str>)> = Vec::with_capacity(agent_role_ids.len() + 1);
    jobs.push((String::new(), None));
    jobs.extend(agent_role_ids.iter().map(|id| (id.clone(), Some(id.as_str()))));
    run_jobs_in_batches(
        cfg,
        tool_recorder,
        workspace_root,
        memory_snippet,
        &jobs,
        max_warmup_threads(),
    )
}

/// 预热并发度：角色数通常是个位数，限制上限只为防「配置里塞了几十个角色」时线程爆炸。
fn max_warmup_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
}

/// 把 `jobs` 按 `max_threads` 分批，每批在 `thread::scope` 内并行；panic 的项按「不产出」跳过。
fn run_jobs_in_batches(
    cfg: &crate::AgentConfig,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
    workspace_root: &std::path::Path,
    memory_snippet: Option<String>,
    jobs: &[(String, Option<&str>)],
    max_threads: usize,
) -> BTreeMap<String, u32> {
    let mut baseline_by_role = BTreeMap::new();
    for batch in jobs.chunks(max_threads.max(1)) {
        let computed: Vec<(String, u32)> = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|(key, role)| {
                    let key = key.clone();
                    let role = *role;
                    let snippet = memory_snippet.clone();
                    scope.spawn(move || {
                        tiktoken_baseline_tokens_for_role(
                            cfg,
                            tool_recorder,
                            workspace_root,
                            snippet,
                            role,
                        )
                        .map(|tokens| (key, tokens))
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok().flatten())
                .collect()
        });
        baseline_by_role.extend(computed);
    }
    baseline_by_role
}

#[cfg(test)]
mod tests {
    use super::{tiktoken_baseline_tokens_for_role, tiktoken_new_session_baselines_by_role};
    use std::sync::Arc;

    fn default_cfg() -> crate::AgentConfig {
        crate::cm_config::load_config(None).expect("embed default")
    }

    fn test_recorder() -> Arc<crate::tool_stats::ToolOutcomeRecorder> {
        Arc::new(crate::tool_stats::ToolOutcomeRecorder::new())
    }

    fn role_ids(cfg: &crate::AgentConfig) -> Vec<String> {
        let mut ids: Vec<String> = cfg.roles_prompts.agent_roles.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// 并行实现必须与串行逐值等价，且多角色输入不漏项。
    #[test]
    fn parallel_baseline_matches_serial_reference() {
        let cfg = default_cfg();
        let recorder = test_recorder();
        // 空临时目录：确保备忘文件不存在，串行参考与并行实现看到同一输入。
        let ws = tempfile::tempdir().expect("tempdir");
        let ids = role_ids(&cfg);
        assert!(!ids.is_empty(), "默认配置应至少有一个角色");

        // 预热一次：把 BPE 编码器与工作区文件读取带进热态，
        // 否则串行侧会独占那份一次性初始化成本，两者时间不可比。
        let _ = tiktoken_baseline_tokens_for_role(&cfg, &recorder, ws.path(), None, Some(&ids[0]));

        let t0 = std::time::Instant::now();
        let mut serial = std::collections::BTreeMap::new();
        for id in &ids {
            if let Some(tokens) =
                tiktoken_baseline_tokens_for_role(&cfg, &recorder, ws.path(), None, Some(id.as_str()))
            {
                serial.insert(id.clone(), tokens);
            }
        }
        let serial_ms = t0.elapsed().as_millis();

        let t1 = std::time::Instant::now();
        let parallel = tiktoken_new_session_baselines_by_role(&cfg, &recorder, ws.path(), &ids);
        let parallel_ms = t1.elapsed().as_millis();
        println!(
            "roles={} serial_ms={serial_ms} parallel_ms={parallel_ms}",
            ids.len()
        );

        for (role, tokens) in &serial {
            assert_eq!(
                parallel.get(role),
                Some(tokens),
                "角色 {role} 的基线必须与串行参考逐值一致"
            );
        }
        assert!(parallel.contains_key(""), "并行结果必须包含 \"\" 无角色档");
    }

    /// 零角色时仍产出 `""` 那档，不应 panic 也不应空转。
    #[test]
    fn empty_role_list_still_yields_blank_role_baseline() {
        let cfg = default_cfg();
        let recorder = test_recorder();
        let ws = tempfile::tempdir().expect("tempdir");
        let out = tiktoken_new_session_baselines_by_role(&cfg, &recorder, ws.path(), &[]);
        assert!(out.contains_key(""), "无角色输入也应算 \"\" 这一档");
        assert_eq!(out.len(), 1, "除 \"\" 外不应有其它键");
    }
}
