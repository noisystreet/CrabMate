export interface StatusData {
  status: string
  model: string
  api_base: string
  max_tokens: number
  temperature: number
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
