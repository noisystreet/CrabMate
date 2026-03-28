/** 与 `GET /status` JSON 对齐；新字段均为可选以兼容旧后端。 */
export interface StatusData {
  status: string
  model: string
  api_base: string
  max_tokens: number
  temperature: number
  /** 默认写入 chat/completions 的整数 seed；未配置时为 undefined */
  llm_seed?: number | null
  /** 当前会话实际加载的工具个数 */
  tool_count?: number
  tool_names?: string[]
  /** 服务端显式注册的分发元数据（其余工具名走同步路径） */
  tool_dispatch_registry?: Array<{
    name: string
    requires_workspace: boolean
    class:
      | 'workflow'
      | 'command_spawn_timeout'
      | 'executable_spawn_timeout'
      | 'weather_spawn_timeout'
      | 'blocking_sync'
  }>
  reflection_default_max_rounds?: number
  /** 与 `[agent] final_plan_requirement` 一致 */
  final_plan_requirement?: 'never' | 'workflow_reflection' | 'always'
  /** 终答规划重写次数上限 */
  plan_rewrite_max_attempts?: number
  /** 阶段1：规划器/执行器运行模式 */
  planner_executor_mode?: 'single_agent' | 'logical_dual_agent'
  /** 为 true 时先无工具规划轮再按 agent_reply_plan 分步执行 */
  staged_plan_execution?: boolean
  /** CLI 是否在无工具规划轮向 stdout 打印模型原文（默认 true；仅 CLI 语义） */
  staged_plan_cli_show_planner_stream?: boolean
  /** 首轮 agent_reply_plan 后是否再跑无工具步骤优化轮（默认 true） */
  staged_plan_optimizer_round?: boolean
  /** 逻辑多规划员份数上限（1–3，默认 1） */
  staged_plan_ensemble_count?: number
  /** SyncDefault 工具沙盒：none | docker */
  sync_default_tool_sandbox_mode?: string
  sync_default_tool_sandbox_docker_image?: string
  /** Docker 沙盒容器 user 解析结果：`uid:gid` 或 `image_default` */
  sync_default_tool_sandbox_docker_user_effective?: string
  /** TUI 启动是否从 .crabmate/tui_session.json 恢复会话（默认 false） */
  tui_load_session_on_start?: boolean
  /** 上下文：system 后最多保留消息条数 */
  max_message_history?: number
  tool_message_max_chars?: number
  context_char_budget?: number
  /** 0 表示未启用 LLM 摘要 */
  context_summary_trigger_chars?: number
  /** 单轮内并行只读工具并发上限 */
  parallel_readonly_tools_max?: number
  /** 单轮 read_file 缓存条数；0 关闭 */
  read_file_turn_cache_max_entries?: number
  /** 进程内对话任务队列（/chat、/chat/stream） */
  chat_queue_max_concurrent?: number
  chat_queue_max_pending?: number
  chat_queue_running?: number
  chat_queue_completed_ok?: number
  chat_queue_completed_cancelled?: number
  chat_queue_completed_err?: number
  chat_queue_recent_jobs?: Array<{
    job_id: number
    kind: string
    ok: boolean
    cancelled?: boolean
    duration_ms: number
    error_preview?: string
  }>
  /** Web `POST /workspace` 允许的根目录个数（未配置多根时为 1） */
  workspace_allowed_roots_count?: number
  /** 队列内正在执行的对话任务之 PER 镜像（无运行中任务时省略或为空数组） */
  per_active_jobs?: Array<{
    job_id: number
    awaiting_plan_rewrite_model: boolean
    plan_rewrite_attempts: number
    require_plan_in_final_content: boolean
  }>
  /** 当前内存会话存储中的会话数量（conversation_id 维度） */
  conversation_store_entries?: number
  /** 长期记忆（与会话/CLI 集成） */
  long_term_memory_enabled?: boolean
  long_term_memory_vector_backend?: string
  long_term_memory_store_ready?: boolean
  long_term_memory_index_errors?: number
  /** Web 新会话首轮是否注入自动生成的项目画像 */
  project_profile_inject_enabled?: boolean
  /** 项目画像注入正文最大字符数（0 表示不生成正文） */
  project_profile_inject_max_chars?: number
  /** 是否启用工具调用解释卡（crabmate_explain_why） */
  tool_call_explain_enabled?: boolean
  tool_call_explain_min_chars?: number
  tool_call_explain_max_chars?: number
}

export interface WorkspaceEntry {
  name: string
  is_dir: boolean
}

export interface WorkspaceData {
  path: string
  entries: WorkspaceEntry[]
  error?: string
}

export interface ChatResponse {
  reply: string
  conversation_id?: string
  /** 非流式 `/chat` 成功落库后的 revision；流式以 SSE `conversation_saved` 为准 */
  conversation_revision?: number | null
}

export interface TaskItem {
  id: string
  title: string
  done: boolean
}

export interface TasksData {
  source?: string
  updated_at?: string
  items: TaskItem[]
}
