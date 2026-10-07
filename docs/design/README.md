# 设计文档索引（`docs/design`）

本目录沉淀 **专题设计**：ADR（架构决策）、设计草案、字段级契约、实施计划与运行手册。
架构总览见 [开发文档](../开发文档.md)，未完成事项见 [待办清单](../待办清单.md)。

**约定**：文件名后缀表示文档形态 —— `*_contract.md` 为字段级接口规格、`*_todo.md` 为实施计划、
`*_runbook.md` 为操作手册；其余多为 ADR 或设计草案。方案落地后请更新文档内状态；
已过期或被取代的文档移入 [`archive/`](archive/)。

## 1. Client / 双仓（8）

| 文档 | 说明 |
|---|---|
| [client_shell_split.md](client_shell_split.md) | ADR：官方 Client 与本仓「只维护 Server」（路径 A） |
| [client_shell_split_todo.md](client_shell_split_todo.md) | 官方 Client 拆分 — 执行计划（路径 A） |
| [client_ui_runtime_split.md](client_ui_runtime_split.md) | Client 承载 UI · Server 默认纯 API（运行时拆分） |
| [client_display_crate_sink.md](client_display_crate_sink.md) | ADR：展示 crate 下沉 Client（执行计划） |
| [client_contract_versioning.md](client_contract_versioning.md) | Client 契约发版与钉版本（Phase 1 + UI 钉清单） |
| [client_compat_matrix.md](client_compat_matrix.md) | Client ↔ Server 兼容矩阵（路径 A） |
| [client_turn_smoke_runbook.md](client_turn_smoke_runbook.md) | 三端真实回合冒烟（B3 Runbook） |
| [vscode_extension.md](vscode_extension.md) | 官方 VS Code 扩展：设计备忘与路线图 |

## 2. 后台工具任务（7）

| 文档 | 说明 |
|---|---|
| [background_tool_jobs.md](background_tool_jobs.md) | ADR：后台工具任务（background tool jobs） |
| [background_tool_jobs_contract.md](background_tool_jobs_contract.md) | 后台工具任务：字段级接口规格 |
| [background_tool_jobs_todo.md](background_tool_jobs_todo.md) | 后台工具任务：实施计划 |
| [background_tool_jobs_output_streaming.md](background_tool_jobs_output_streaming.md) | ADR：后台工具任务实时输出流 |
| [background_tool_jobs_output_streaming_contract.md](background_tool_jobs_output_streaming_contract.md) | 实时输出流：字段级接口规格 |
| [background_tool_jobs_output_streaming_todo.md](background_tool_jobs_output_streaming_todo.md) | 实时输出流：实施计划 |
| [long_running_tool_execution_todo.md](long_running_tool_execution_todo.md) | 长耗时工具执行：待办 |

## 3. 工具系统（3）

| 文档 | 说明 |
|---|---|
| [tool_calling_evolution.md](tool_calling_evolution.md) | 工具调用层：对标开源 Agent 的演进方向（含薄封装收敛 §8.5） |
| [tool_thin_wrapper_candidates.md](tool_thin_wrapper_candidates.md) | 工具薄 CLI 封装：剩余家族候选盘点与判定（§8.5 配套） |
| [tool_management_api.md](tool_management_api.md) | 设计草案：工具管理 API 补齐 |

## 4. 回合 / Agent 运行时（8）

| 文档 | 说明 |
|---|---|
| [agent_state_management.md](agent_state_management.md) | CrabMate Agent 状态管理设计 |
| [agent_turn_split.md](agent_turn_split.md) | Agent turn 拆分 |
| [agent_self_evolution_roadmap.md](agent_self_evolution_roadmap.md) | Server Agent 自进化能力增强路线 |
| [turn_host_decouple.md](turn_host_decouple.md) | 回合宿主解耦（Turn Host Decouple） |
| [turn_runtime_placement.md](turn_runtime_placement.md) | ADR：回合执行面落点（`crabmate-turn-runtime`） |
| [run_loop_state_ownership.md](run_loop_state_ownership.md) | 回合可变状态归属 |
| [per_state_machine_consolidation.md](per_state_machine_consolidation.md) | PER 编排：用显式状态机收拢分支 |
| [audience_critic_role.md](audience_critic_role.md) | 观众角色（侧向点评）：设计草案 |

## 5. 上下文 / 提示词（5）

| 文档 | 说明 |
|---|---|
| [context_trimming_scheme.md](context_trimming_scheme.md) | 上下文裁剪方案设计 |
| [context_window_management_react_pruning.md](context_window_management_react_pruning.md) | 上下文窗口管理（ReAct 循环裁剪） |
| [context_window_token_aware_compaction.md](context_window_token_aware_compaction.md) | ADR：Token 主导、交互组完整的压缩策略 |
| [design_prompt_caching.md](design_prompt_caching.md) | Prompt Caching 支持 — 设计文档 |
| [system_prompt_assembly.md](system_prompt_assembly.md) | 系统提示词动态组装 |

## 6. 记忆 / 会话（5）

| 文档 | 说明 |
|---|---|
| [memory_management_api.md](memory_management_api.md) | 设计草案：长期记忆（memory）管理 API 补齐 |
| [memory_todo.md](memory_todo.md) | 长期记忆可视化面板 |
| [summarize_experience.md](summarize_experience.md) | 经验总结工具 `summarize_experience` |
| [summarize_experience_todo.md](summarize_experience_todo.md) | `summarize_experience` 待实现功能 |
| [conversation_management_api.md](conversation_management_api.md) | 设计草案：会话（conversation）管理 API 补齐 |

## 7. Web / UI 宿主（6）

| 文档 | 说明 |
|---|---|
| [web_host_extract.md](web_host_extract.md) | Web 宿主提取（modular monolith） |
| [web_host_p5_placement.md](web_host_p5_placement.md) | ADR：P5 — `chat_job_queue` / chat handler 贴近 web-host |
| [web_api_integration.md](web_api_integration.md) | Web API 第三方集成：架构备忘与增强方向 |
| [web_theme_presets.md](web_theme_presets.md) | Web 前端：多套预设主题（扩展 `data-theme`） |
| [web_tui_stream_to_opencode_style.md](web_tui_stream_to_opencode_style.md) | Web 聊天：OpenCode / OpenClaw 式流式跟底演进 |
| [web_ui_todo.md](web_ui_todo.md) | Web UI 未来功能规划 |

## 8. 工程 / 基础设施（6）

| 文档 | 说明 |
|---|---|
| [crate_dep_policy.md](crate_dep_policy.md) | Crate 依赖策略（降低耦合） |
| [crates_io_single_package.md](crates_io_single_package.md) | ADR：单包 `crabmate` 发布到 crates.io |
| [user_data_dir.md](user_data_dir.md) | 本机用户数据目录（`~/.local/share/crabmate`）设计 |
| [e2e_real_llm_testing_architecture.md](e2e_real_llm_testing_architecture.md) | E2E 真实 LLM 自动化测试架构 |
| [ide_mode_roadmap.md](ide_mode_roadmap.md) | Web / 桌面内置 IDE 模式：完善规划 |
| [server_api_completeness.md](server_api_completeness.md) | ADR：Server API 完备度边界与演进基线 |

## 已归档（`archive/`）

仅作历史参考，内容可能已被上表文档取代：

- [TUI_CLI改造实施步骤.md](archive/TUI_CLI改造实施步骤.md) — CLI → TUI 改造实施步骤（与 Web 布局对齐）
- [tui_align_tauri_display.md](archive/tui_align_tauri_display.md) — 终端 TUI 对齐 Tauri / Web 展示规划
- [tui_chat_display_ownership.md](archive/tui_chat_display_ownership.md) — TUI 中区展示所有权（ADR）
- [分层多智能体架构.md](archive/分层多智能体架构.md) — 分层多 Agent 协作架构：Manager + Operator + 多 Agent 群体
- [orchestration_decision_engine.md](archive/orchestration_decision_engine.md) — 编排决策引擎：从二元门控到多因子评分
- [Web界面美化设计.md](archive/Web界面美化设计.md) — Web 界面美化（Leptos 前端；UI 已迁 Client 仓）
- [代码库索引方案.md](archive/代码库索引方案.md) — 工作区统一代码索引与增量缓存（已落地，实现见开发文档 / 工具说明）
- [CODEBASE_INDEX_PLAN.md](archive/CODEBASE_INDEX_PLAN.md) — 上者英文镜像
