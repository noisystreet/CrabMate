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
