import type { StatusData, WorkspaceData, ChatResponse, TasksData } from './types'

const base = ''

export async function fetchStatus(): Promise<StatusData> {
  const r = await fetch(`${base}/status`)
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

export async function fetchWorkspace(dirPath?: string | null): Promise<WorkspaceData> {
  const url = dirPath
    ? `${base}/workspace?path=${encodeURIComponent(dirPath)}`
    : `${base}/workspace`
  const r = await fetch(url)
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

/** 设置后端当前工作区路径（与前端工作区一致，Agent 和文件 API 将使用该路径）；path 为空表示使用服务端配置默认 */
export async function setWorkspacePath(path: string): Promise<{ ok: boolean; path: string }> {
  const r = await fetch(`${base}/workspace`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path: path.trim() || undefined }),
  })
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

export interface WorkspacePickResponse {
  path: string | null
}

export async function fetchWorkspacePick(): Promise<WorkspacePickResponse> {
  const r = await fetch(`${base}/workspace/pick`)
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

/** 读取工作区内文件内容，path 为文件完整路径（与工作区列表 data.path 同源） */
export async function fetchWorkspaceFile(path: string): Promise<{ content: string; error?: string }> {
  const r = await fetch(`${base}/workspace/file?path=${encodeURIComponent(path)}`)
  const data = await r.json().catch(() => ({}))
  if (!r.ok) throw new Error((data as { error?: string }).error || r.statusText)
  return data as { content: string; error?: string }
}

/** 在工作区内创建或覆盖文件，path 为文件完整路径 */
export async function writeWorkspaceFile(path: string, content: string): Promise<{ error?: string }> {
  const r = await fetch(`${base}/workspace/file`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path, content }),
  })
  const data = await r.json().catch(() => ({}))
  if (!r.ok) throw new Error((data as { error?: string }).error || r.statusText)
  return data as { error?: string }
}

/** 删除工作区内文件，path 为文件完整路径 */
export async function deleteWorkspaceFile(path: string): Promise<{ error?: string }> {
  const r = await fetch(`${base}/workspace/file?path=${encodeURIComponent(path)}`, {
    method: 'DELETE',
  })
  const data = await r.json().catch(() => ({}))
  if (!r.ok) throw new Error((data as { error?: string }).error || r.statusText)
  return data as { error?: string }
}

/** 获取当前任务清单（位于工作区根目录的 tasks.json） */
export async function fetchTasks(): Promise<TasksData> {
  const r = await fetch(`${base}/tasks`)
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

/** 覆盖保存任务清单（写入工作区根目录的 tasks.json） */
export async function saveTasks(data: TasksData): Promise<TasksData> {
  const r = await fetch(`${base}/tasks`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  })
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

export async function sendChat(message: string): Promise<ChatResponse> {
  const r = await fetch(`${base}/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message }),
  })
  const data = await r.json().catch(() => ({}))
  if (!r.ok) throw new Error((data as { message?: string }).message || r.statusText)
  return data as ChatResponse
}

export interface WorkspaceSearchRequest {
  pattern: string
  path?: string | null
  max_results?: number
  case_insensitive?: boolean
  ignore_hidden?: boolean
}

export interface WorkspaceSearchResponse {
  output: string
}

/** 在工作区内搜索文件内容（基于后端 grep 工具），path 为空则从工作区根目录开始；否则从指定目录递归搜索 */
export async function searchWorkspace(body: WorkspaceSearchRequest): Promise<WorkspaceSearchResponse> {
  const r = await fetch(`${base}/workspace/search`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  const data = await r.json().catch(() => ({}))
  if (!r.ok) {
    throw new Error((data as { error?: string }).error || r.statusText)
  }
  return data as WorkspaceSearchResponse
}

/** 流式 chat：POST /chat/stream，通过 onDelta 逐段接收内容，onDone 结束时调用，失败时 onError；收到 workspace_changed 时调用 onWorkspaceChanged 以刷新工作区 */
export async function sendChatStream(
  message: string,
  callbacks: {
    onDelta: (text: string) => void
    onDone: () => void
    onError: (err: string) => void
    onWorkspaceChanged?: () => void
    onToolCall?: (info: { name: string; summary: string }) => void
    onToolStatusChange?: (running: boolean) => void
    onToolResult?: (info: { name: string; output: string }) => void
  },
  signal?: AbortSignal,
): Promise<void> {
  const r = await fetch(`${base}/chat/stream`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message }),
    signal,
  })
  if (!r.ok) {
    const data = await r.json().catch(() => ({}))
    callbacks.onError((data as { message?: string }).message || r.statusText)
    return
  }
  const reader = r.body?.getReader()
  if (!reader) {
    callbacks.onError('无法读取响应流')
    return
  }
  const decoder = new TextDecoder()
  let buffer = ''
  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      const parts = buffer.split('\n\n')
      buffer = parts.pop() ?? ''
      for (const block of parts) {
        const dataLines = block.split('\n').filter((l) => l.startsWith('data: '))
        const data = dataLines.map((l) => l.slice(6).replace(/^\s+/, '')).join('\n').trim()
        if (!data) continue
        try {
          const parsed = JSON.parse(data) as {
            error?: string
            workspace_changed?: boolean
            tool_call?: { name?: string; summary?: string }
            tool_running?: boolean
            tool_result?: { name?: string; output?: string }
          }
          if (parsed.error != null) {
            callbacks.onError(parsed.error)
            return
          }
          if (parsed.workspace_changed === true) {
            callbacks.onWorkspaceChanged?.()
            continue
          }
          if (parsed.tool_call?.summary) {
            callbacks.onToolCall?.({
              name: parsed.tool_call.name || '',
              summary: parsed.tool_call.summary,
            })
            continue
          }
          if (typeof parsed.tool_running === 'boolean') {
            callbacks.onToolStatusChange?.(parsed.tool_running)
            continue
          }
          if (parsed.tool_result?.output != null) {
            callbacks.onToolResult?.({
              name: parsed.tool_result.name || '',
              output: parsed.tool_result.output,
            })
            continue
          }
        } catch {
          // 非 JSON，当作纯文本 delta
        }
        if (data !== '[DONE]') callbacks.onDelta(data)
      }
    }
    if (buffer.trim()) {
      const dataLines = buffer.split('\n').filter((l) => l.startsWith('data: '))
      const data = dataLines.map((l) => l.slice(6).replace(/^\s+/, '')).join('\n').trim()
      if (data && data !== '[DONE]') {
        try {
          const parsed = JSON.parse(data) as {
            error?: string
            workspace_changed?: boolean
            tool_call?: { name?: string; summary?: string }
            tool_running?: boolean
            tool_result?: { name?: string; output?: string }
          }
          if (parsed.error != null) callbacks.onError(parsed.error)
          else if (parsed.workspace_changed === true) callbacks.onWorkspaceChanged?.()
          else if (parsed.tool_call?.summary) {
            callbacks.onToolCall?.({
              name: parsed.tool_call.name || '',
              summary: parsed.tool_call.summary,
            })
          } else if (typeof parsed.tool_running === 'boolean') {
            callbacks.onToolStatusChange?.(parsed.tool_running)
          } else if (parsed.tool_result?.output != null) {
            callbacks.onToolResult?.({
              name: parsed.tool_result.name || '',
              output: parsed.tool_result.output,
            })
          } else callbacks.onDelta(data)
        } catch {
          callbacks.onDelta(data)
        }
      }
    }
    callbacks.onDone()
  } catch (e) {
    // 若是主动取消（AbortError），静默返回；否则按错误处理
    if (e instanceof DOMException && e.name === 'AbortError') {
      return
    }
    callbacks.onError(e instanceof Error ? e.message : '流式读取失败')
  }
}
