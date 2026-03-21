/** 与 `GET /status` JSON 对齐；新字段均为可选以兼容旧后端。 */
export interface StatusData {
  status: string
  model: string
  api_base: string
  max_tokens: number
  temperature: number
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
  /** 上下文：system 后最多保留消息条数 */
  max_message_history?: number
  tool_message_max_chars?: number
  context_char_budget?: number
  /** 0 表示未启用 LLM 摘要 */
  context_summary_trigger_chars?: number
  /** 进程内对话任务队列（/chat、/chat/stream） */
  chat_queue_max_concurrent?: number
  chat_queue_max_pending?: number
  chat_queue_running?: number
  chat_queue_completed_ok?: number
  chat_queue_completed_err?: number
  chat_queue_recent_jobs?: Array<{
    job_id: number
    kind: string
    ok: boolean
    duration_ms: number
    error_preview?: string
  }>
  /** 队列内正在执行的对话任务之 PER 镜像（无运行中任务时省略或为空数组） */
  per_active_jobs?: Array<{
    job_id: number
    awaiting_plan_rewrite_model: boolean
    plan_rewrite_attempts: number
    require_plan_in_final_content: boolean
  }>
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
