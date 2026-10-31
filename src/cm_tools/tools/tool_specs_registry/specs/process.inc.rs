[
ToolSpec {
            name: tool_names::BACKGROUND_JOB,
            description: "查询后台工具任务（只读，不走白名单审批）。**`action` 必填**：`status` 按 `tool_job_id` 查状态与（终态）输出；`list` 列出当前工作区任务（最新创建在前，可选 `limit`，默认 20、上限 100）；`output` 按 `tool_job_id` 增量拉取输出（`tail -f` 语义，可选 `cursor` = 上次返回的 `next_cursor`）。**`status` / `output` 必须传 `tool_job_id`**。与本 `action` 无关的字段会被忽略，但 `limit` 无论 action 都受 1–100 约束（超出即参数校验失败）。用于 `run_command` 以 `async: true` 启动的长任务。取消任务请用 `background_job_cancel`；要**等任务结束**请用 `background_job_wait`（服务端内部等待，不必自己反复轮询）。",
            category: ToolCategory::Development,
            tags: &[dev_tag::GENERAL],
            parameters: schema_of::<args::BackgroundJobArgs>,
            runner: ToolRunner::Typed(runner_background_job_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_background_job),
        },
        builtin_tool! {
            name: tool_names::BACKGROUND_JOB_CANCEL,
            args: args::BackgroundJobCancelArgs,
            category: Development,
            tags: [dev_tag::GENERAL],
            runner: runner_background_job_cancel_try,
            summary: dynamic,
            description: "取消后台工具任务（终止语义，非发起执行）。queued 直接转移为 cancelled；running 置取消标记；已是 cancelled 幂等成功；其它终态不可取消。",
        },
        ToolSpec {
            name: tool_names::BACKGROUND_JOB_WAIT,
            description: "等待后台任务满足条件后返回（**只读**；扫描在服务端完成，省掉多轮 `background_job status` 的往返）。`until` 默认 `exit`（等到终态）；`until=output_matches` 时必须给 `output_matches`（**子串**，非正则，≤256 字节），输出出现即返回（例如等 dev server 打印 `listening on`）——省略 `until` 但给了 `output_matches` 时按 `output_matches` 处理。`timeout_secs` 是本次等待的墙钟（钳制 1–600，默认 60）：**超时不取消任务**，任务保持原状态，可继续等待，或改用 `background_job`（`action=output`）取增量；任务被取消（`background_job_cancel` 或用户停止回合）时等待会提前结束并注明「已请求取消」。本工具会阻塞至多 `timeout_secs` 秒，**不要与其它工具并排发起**。返回体把「等待结果」与「任务状态」分开写：任务自身的 `timed_out` 与本次等待超时是两件事。终态记录只保留 `background_job_ttl_secs` + `background_job_result_grace_secs`（默认 1 天 + 300 秒），过期后再等会得到「已过保留时长」错误。",
            category: ToolCategory::Development,
            tags: &[dev_tag::GENERAL],
            parameters: schema_of::<args::BackgroundJobWaitArgs>,
            runner: ToolRunner::Typed(runner_background_job_wait_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_background_job_wait),
        },
        // ── 代码度量与分析 ──────────────────────────────────
]
