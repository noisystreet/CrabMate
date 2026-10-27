[
ToolSpec {
            name: "background_job",
            description: "查询后台工具任务（只读，不走白名单审批）。**`action` 必填**：`status` 按 `tool_job_id` 查状态与（终态）输出；`list` 列出当前工作区任务（最新创建在前，可选 `limit`，默认 20、上限 100）；`output` 按 `tool_job_id` 增量拉取输出（`tail -f` 语义，可选 `cursor` = 上次返回的 `next_cursor`）。**`status` / `output` 必须传 `tool_job_id`**。与本 `action` 无关的字段会被忽略，但 `limit` 无论 action 都受 1–100 约束（超出即参数校验失败）。用于 `run_command` 以 `async: true` 启动的长任务。取消任务请用 `background_job_cancel`。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::BackgroundJobArgs>,
            runner: ToolRunner::Typed(runner_background_job_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_background_job),
        },
        ToolSpec {
            name: "background_job_cancel",
            description: "取消后台工具任务（终止语义，非发起执行）。queued 直接转移为 cancelled；running 置取消标记；已是 cancelled 幂等成功；其它终态不可取消。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::BackgroundJobCancelArgs>,
            runner: ToolRunner::Typed(runner_background_job_cancel_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_background_job_cancel),
        },
        // ── 代码度量与分析 ──────────────────────────────────
]
