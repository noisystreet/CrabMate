import type { StatusData, WorkspaceData, ChatResponse, TasksData } from './types'

const base = ''
const WEB_API_BEARER_TOKEN_KEY = 'crabmate-api-bearer-token'

function getStoredWebApiBearerToken(): string | null {
  if (typeof window === 'undefined') return null
  const v = window.localStorage.getItem(WEB_API_BEARER_TOKEN_KEY)
  const t = (v || '').trim()
  return t ? t : null
}

export type ApiErrorKind = 'http' | 'timeout' | 'network' | 'abort' | 'parse' | 'unknown'

export class ApiError extends Error {
  kind: ApiErrorKind
  status?: number
  url?: string
  details?: unknown
  constructor(message: string, info: { kind: ApiErrorKind; status?: number; url?: string; details?: unknown }) {
    super(message)
    this.name = 'ApiError'
    this.kind = info.kind
    this.status = info.status
    this.url = info.url
    this.details = info.details
  }
}

type RequestCacheOptions = {
  /** 缓存存活时间（毫秒），仅对 GET 生效；0/undefined 表示不缓存 */
  ttlMs?: number
  /** 若有旧值则先返回旧值，同时后台刷新更新缓存（仅 GET 生效） */
  staleWhileRevalidate?: boolean
}

type RequestOptions = Omit<RequestInit, 'body'> & {
  /** 超时（毫秒）；默认 15s */
  timeoutMs?: number
  /** 重试次数（不含首次）；默认 0 */
  retries?: number
  /** 初始退避（毫秒）；默认 250ms */
  retryBaseDelayMs?: number
  /** GET in-flight 去重；默认 true */
  dedupe?: boolean
  /** 仅当 method 为 GET 时生效（避免与 RequestInit.cache 冲突） */
  clientCache?: RequestCacheOptions
  /** JSON body（自动 Content-Type） */
  json?: unknown
  /** 期望返回 JSON（默认 true）；false 则返回 text */
  expectJson?: boolean
}

type CacheEntry = { value: unknown; expiresAt: number }
const inflight = new Map<string, Promise<unknown>>()
const cacheStore = new Map<string, CacheEntry>()

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms))
}

function makeKey(url: string, init: RequestOptions): string {
  const method = (init.method || 'GET').toUpperCase()
  const bodyKey = init.json !== undefined ? JSON.stringify(init.json) : ''
  return `${method} ${url} ${bodyKey}`
}

function shouldRetry(err: unknown): boolean {
  if (err instanceof ApiError) {
    if (err.kind === 'abort') return false
    if (err.kind === 'timeout') return true
    if (err.kind === 'network') return true
    if (err.kind === 'http' && err.status != null) {
      return [408, 429, 500, 502, 503, 504].includes(err.status)
    }
    return false
  }
  return true
}

async function requestImpl<T>(url: string, init: RequestOptions = {}): Promise<T> {
  const method = (init.method || 'GET').toUpperCase()
  const timeoutMs = init.timeoutMs ?? 15000
  const expectJson = init.expectJson ?? true
  const headers = new Headers(init.headers)
  const bearerToken = getStoredWebApiBearerToken()
  if (bearerToken && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${bearerToken}`)
  }
  let body: BodyInit | undefined
  if (init.json !== undefined) {
    if (!headers.has('Content-Type')) headers.set('Content-Type', 'application/json')
    body = JSON.stringify(init.json)
  }

  // 超时控制：把 timeout 合并到一个 AbortController 中
  const outerSignal = init.signal
  const controller = new AbortController()
  const onAbort = () => controller.abort()
  if (outerSignal) {
    if (outerSignal.aborted) controller.abort()
    else outerSignal.addEventListener('abort', onAbort, { once: true })
  }
  const timer = window.setTimeout(() => controller.abort(), timeoutMs)

  try {
    const r = await fetch(url, {
      ...init,
      method,
      headers,
      body,
      signal: controller.signal,
    })

    const isJson = (r.headers.get('content-type') || '').includes('application/json')
    const data = expectJson
      ? await r.json().catch(() => {
          throw new ApiError('响应解析失败', { kind: 'parse', status: r.status, url })
        })
      : await r.text().catch(() => {
          throw new ApiError('响应解析失败', { kind: 'parse', status: r.status, url })
        })

    if (!r.ok) {
      const msg =
        (data && typeof data === 'object' && 'error' in (data as any) && (data as any).error) ||
        (data && typeof data === 'object' && 'message' in (data as any) && (data as any).message) ||
        r.statusText ||
        '请求失败'
      throw new ApiError(String(msg), { kind: 'http', status: r.status, url, details: isJson ? data : undefined })
    }

    return data as T
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      // 无法区分是 timeout 还是外部 abort，这里优先按 timeout 判断
      if (outerSignal?.aborted) throw new ApiError('请求已取消', { kind: 'abort', url })
      throw new ApiError('请求超时', { kind: 'timeout', url })
    }
    if (e instanceof ApiError) throw e
    throw new ApiError(e instanceof Error ? e.message : '网络异常', { kind: 'network', url, details: e })
  } finally {
    window.clearTimeout(timer)
    if (outerSignal) outerSignal.removeEventListener('abort', onAbort)
  }
}

export async function request<T>(path: string, init: RequestOptions = {}): Promise<T> {
  const url = `${base}${path}`
  const method = (init.method || 'GET').toUpperCase()
  const key = makeKey(url, init)

  // GET 缓存 / SWR
  if (method === 'GET' && init.clientCache?.ttlMs && init.clientCache.ttlMs > 0) {
    const entry = cacheStore.get(key)
    const now = Date.now()
    if (entry && entry.expiresAt > now) {
      return entry.value as T
    }
    if (entry && init.clientCache.staleWhileRevalidate) {
      // 后台刷新（去重）
      void request<T>(path, {
        ...init,
        clientCache: { ...(init.clientCache || {}), staleWhileRevalidate: false },
      }).catch(() => {})
      return entry.value as T
    }
  }

  // GET in-flight 去重
  if (method === 'GET' && (init.dedupe ?? true)) {
    const existing = inflight.get(key)
    if (existing) return existing as Promise<T>
  }

  const doFetch = async (): Promise<T> => {
    const retries = init.retries ?? 0
    const baseDelay = init.retryBaseDelayMs ?? 250
    let attempt = 0
    while (true) {
      try {
        const data = await requestImpl<T>(url, init)
        if (method === 'GET' && init.clientCache?.ttlMs && init.clientCache.ttlMs > 0) {
          cacheStore.set(key, { value: data, expiresAt: Date.now() + init.clientCache.ttlMs })
        }
        return data
      } catch (e) {
        if (attempt >= retries || !shouldRetry(e)) throw e
        const delay = Math.min(5000, baseDelay * 2 ** attempt)
        attempt += 1
        await sleep(delay)
      }
    }
  }

  const p = doFetch().finally(() => {
    inflight.delete(key)
  })
  if (method === 'GET' && (init.dedupe ?? true)) inflight.set(key, p as Promise<unknown>)
  return p
}

export async function fetchStatus(): Promise<StatusData> {
  return request<StatusData>('/status', {
    timeoutMs: 5000,
    retries: 1,
    clientCache: { ttlMs: 30000, staleWhileRevalidate: true },
  })
}

export async function fetchWorkspace(dirPath?: string | null): Promise<WorkspaceData> {
  const url = dirPath
    ? `/workspace?path=${encodeURIComponent(dirPath)}`
    : `/workspace`
  return request<WorkspaceData>(url, { timeoutMs: 15000, retries: 1 })
}

/** 设置后端当前工作区路径（与前端工作区一致，Agent 和文件 API 将使用该路径）；path 为空表示使用服务端配置默认。路径须落在服务端允许的根目录下，否则返回 403。 */
export async function setWorkspacePath(path: string): Promise<{ ok: boolean; path: string }> {
  return request<{ ok: boolean; path: string; error?: string }>('/workspace', {
    method: 'POST',
    timeoutMs: 15000,
    retries: 0,
    json: { path: path.trim() || undefined },
  })
}

export interface WorkspacePickResponse {
  path: string | null
}

export async function fetchWorkspacePick(): Promise<WorkspacePickResponse> {
  return request<WorkspacePickResponse>('/workspace/pick', { timeoutMs: 15000, retries: 0 })
}

/** 读取工作区内文件内容，path 为文件完整路径（与工作区列表 data.path 同源） */
export async function fetchWorkspaceFile(path: string): Promise<{ content: string; error?: string }> {
  return request<{ content: string; error?: string }>(`/workspace/file?path=${encodeURIComponent(path)}`, {
    timeoutMs: 15000,
    retries: 0,
  })
}

/** 在工作区内创建或覆盖文件，path 为文件完整路径 */
export async function writeWorkspaceFile(path: string, content: string): Promise<{ error?: string }> {
  return request<{ error?: string }>('/workspace/file', {
    method: 'POST',
    timeoutMs: 15000,
    retries: 0,
    json: { path, content },
  })
}

/** 删除工作区内文件，path 为文件完整路径 */
export async function deleteWorkspaceFile(path: string): Promise<{ error?: string }> {
  return request<{ error?: string }>(`/workspace/file?path=${encodeURIComponent(path)}`, {
    method: 'DELETE',
    timeoutMs: 15000,
    retries: 0,
  })
}

/** 获取当前任务清单（位于工作区根目录的 tasks.json） */
export async function fetchTasks(): Promise<TasksData> {
  return request<TasksData>('/tasks', { timeoutMs: 15000, retries: 1 })
}

/** 覆盖保存任务清单（写入工作区根目录的 tasks.json） */
export async function saveTasks(data: TasksData): Promise<TasksData> {
  return request<TasksData>('/tasks', {
    method: 'POST',
    timeoutMs: 15000,
    retries: 0,
    json: data,
  })
}

export async function sendChat(message: string, conversationId?: string): Promise<ChatResponse> {
  return request<ChatResponse>('/chat', {
    method: 'POST',
    timeoutMs: 60000,
    retries: 0,
    json: conversationId ? { message, conversation_id: conversationId } : { message },
  })
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

export interface ToolResultInfo {
  name: string
  /** 与后端 `summarize_tool_call` 同源；与 output 同帧到达，用于「先摘要后实际输出」 */
  summary?: string
  output: string
  ok?: boolean
  exit_code?: number
  error_code?: string
  stdout?: string
  stderr?: string
}

export interface CommandApprovalRequestInfo {
  command: string
  args: string
  allowlistKey?: string
}

/** 在工作区内搜索文件内容（基于后端 grep 工具），path 为空则从工作区根目录开始；否则从指定目录递归搜索 */
export async function searchWorkspace(body: WorkspaceSearchRequest): Promise<WorkspaceSearchResponse> {
  return request<WorkspaceSearchResponse>('/workspace/search', {
    method: 'POST',
    timeoutMs: 30000,
    retries: 0,
    json: body,
  })
}

export interface UploadFileInfo {
  url: string
  filename: string
  mime: string
  size: number
}

export interface UploadResponseBody {
  files: UploadFileInfo[]
}

export type UploadProgress = { loaded: number; total: number; percent: number }

export async function uploadFiles(
  files: File[],
  opts?: { signal?: AbortSignal; onProgress?: (p: UploadProgress) => void; timeoutMs?: number },
): Promise<UploadResponseBody> {
  const url = `${base}/upload`
  const timeoutMs = opts?.timeoutMs ?? 5 * 60_000

  return await new Promise<UploadResponseBody>((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    const form = new FormData()
    for (const f of files) form.append('file', f, f.name)

    const onAbort = () => xhr.abort()
    if (opts?.signal) {
      if (opts.signal.aborted) xhr.abort()
      else opts.signal.addEventListener('abort', onAbort, { once: true })
    }

    xhr.open('POST', url, true)
    xhr.responseType = 'json'
    xhr.timeout = timeoutMs
    const bearerToken = getStoredWebApiBearerToken()
    if (bearerToken) {
      xhr.setRequestHeader('Authorization', `Bearer ${bearerToken}`)
    }

    xhr.upload.onprogress = (e) => {
      if (!opts?.onProgress) return
      if (!e.lengthComputable) return
      const loaded = e.loaded
      const total = e.total || 1
      const percent = Math.max(0, Math.min(100, Math.round((loaded / total) * 100)))
      opts.onProgress({ loaded, total, percent })
    }

    const cleanup = () => {
      if (opts?.signal) opts.signal.removeEventListener('abort', onAbort)
    }

    xhr.onerror = () => {
      cleanup()
      reject(new ApiError('网络异常', { kind: 'network', url }))
    }
    xhr.ontimeout = () => {
      cleanup()
      reject(new ApiError('请求超时', { kind: 'timeout', url }))
    }
    xhr.onabort = () => {
      cleanup()
      reject(new ApiError('请求已取消', { kind: 'abort', url }))
    }
    xhr.onload = () => {
      cleanup()
      const status = xhr.status
      const data = xhr.response ?? (() => {
        try {
          return JSON.parse(xhr.responseText || '{}')
        } catch {
          return {}
        }
      })()
      if (status < 200 || status >= 300) {
        const msg = (data as { message?: string }).message || xhr.statusText || '上传失败'
        reject(new ApiError(String(msg), { kind: 'http', status, url, details: data }))
        return
      }
      resolve(data as UploadResponseBody)
    }

    xhr.send(form)
  })
}

export interface DeleteUploadsResponseBody {
  deleted: string[]
  skipped: string[]
}

export async function deleteUploads(urls: string[]): Promise<DeleteUploadsResponseBody> {
  return request<DeleteUploadsResponseBody>('/uploads/delete', {
    method: 'POST',
    timeoutMs: 15000,
    retries: 0,
    json: { urls },
  })
}

/** 与后端 `src/sse/protocol.rs`（`SsePayload` 等）控制面字段对齐；与 `src/sse/line.rs`（`classify_agent_sse_line`）的 Rust 分类逻辑保持语义一致。 */
type SseControlPayload = {
  v?: number
  error?: string
  code?: string
  plan_required?: boolean
  workspace_changed?: boolean
  tool_call?: { name?: string; summary?: string }
  tool_running?: boolean
  parsing_tool_calls?: boolean
  tool_result?: {
    name?: string
    summary?: string
    output?: string
    ok?: boolean
    exit_code?: number
    error_code?: string
    stdout?: string
    stderr?: string
  }
  /** 分阶段规划进度文案（可多行）；TUI 展示；Web 吞掉不当下文 */
  staged_plan_notice?: string
  staged_plan_notice_clear?: boolean
  staged_plan_started?: {
    plan_id?: string
    total_steps?: number
  }
  staged_plan_step_started?: {
    plan_id?: string
    step_id?: string
    step_index?: number
    total_steps?: number
    description?: string
  }
  staged_plan_step_finished?: {
    plan_id?: string
    step_id?: string
    step_index?: number
    total_steps?: number
    /** `ok` / `cancelled` / `failed` */
    status?: string
  }
  staged_plan_finished?: {
    plan_id?: string
    total_steps?: number
    completed_steps?: number
    /** `ok` / `cancelled` / `failed` */
    status?: string
  }
  /** 分阶段规划聊天区分隔线：`true` 短、`false` 长 */
  chat_ui_separator?: boolean
  command_approval_request?: {
    command?: string
    args?: string
    allowlist_key?: string
  }
}

/** 解析单条 `data:` 合并后的字符串：若为控制面 JSON 则调用 callbacks 并返回 `stop` | `handled`；否则返回 `plain`（由调用方按正文 delta 处理）。 */
function tryDispatchSseControlPayload(
  data: string,
  callbacks: {
    onError: (err: string) => void
    onWorkspaceChanged?: () => void
    onToolCall?: (info: { name: string; summary: string }) => void
    onToolStatusChange?: (running: boolean) => void
    onParsingToolCallsChange?: (parsing: boolean) => void
    onToolResult?: (info: ToolResultInfo) => void
    onCommandApprovalRequest?: (info: CommandApprovalRequestInfo) => void
    onPlanRequired?: () => void
    onStagedPlanStarted?: (info: { planId: string; totalSteps: number }) => void
    onStagedPlanStepStarted?: (info: {
      planId: string
      stepId: string
      stepIndex: number
      totalSteps: number
      description: string
    }) => void
    onStagedPlanStepFinished?: (info: {
      planId: string
      stepId: string
      stepIndex: number
      totalSteps: number
      status: string
    }) => void
    onStagedPlanFinished?: (info: {
      planId: string
      totalSteps: number
      completedSteps: number
      status: string
    }) => void
    /** `true` 短分隔线，`false` 长分隔线（分阶段规划队列） */
    onChatUiSeparator?: (short: boolean) => void
  },
): 'stop' | 'handled' | 'plain' {
  try {
    const parsed = JSON.parse(data) as SseControlPayload
    if (parsed.error != null) {
      callbacks.onError(
        parsed.code != null ? `${parsed.error} (${parsed.code})` : parsed.error,
      )
      return 'stop'
    }
    if (parsed.plan_required === true) {
      callbacks.onPlanRequired?.()
      return 'handled'
    }
    if (parsed.staged_plan_started != null) {
      callbacks.onStagedPlanStarted?.({
        planId: parsed.staged_plan_started.plan_id || '',
        totalSteps: parsed.staged_plan_started.total_steps || 0,
      })
      return 'handled'
    }
    if (parsed.staged_plan_step_started != null) {
      callbacks.onStagedPlanStepStarted?.({
        planId: parsed.staged_plan_step_started.plan_id || '',
        stepId: parsed.staged_plan_step_started.step_id || '',
        stepIndex: parsed.staged_plan_step_started.step_index || 0,
        totalSteps: parsed.staged_plan_step_started.total_steps || 0,
        description: parsed.staged_plan_step_started.description || '',
      })
      return 'handled'
    }
    if (parsed.staged_plan_step_finished != null) {
      callbacks.onStagedPlanStepFinished?.({
        planId: parsed.staged_plan_step_finished.plan_id || '',
        stepId: parsed.staged_plan_step_finished.step_id || '',
        stepIndex: parsed.staged_plan_step_finished.step_index || 0,
        totalSteps: parsed.staged_plan_step_finished.total_steps || 0,
        status: parsed.staged_plan_step_finished.status || '',
      })
      return 'handled'
    }
    if (parsed.staged_plan_finished != null) {
      callbacks.onStagedPlanFinished?.({
        planId: parsed.staged_plan_finished.plan_id || '',
        totalSteps: parsed.staged_plan_finished.total_steps || 0,
        completedSteps: parsed.staged_plan_finished.completed_steps || 0,
        status: parsed.staged_plan_finished.status || '',
      })
      return 'handled'
    }
    if (parsed.workspace_changed === true) {
      callbacks.onWorkspaceChanged?.()
      return 'handled'
    }
    if (parsed.tool_call?.summary) {
      callbacks.onToolCall?.({
        name: parsed.tool_call.name || '',
        summary: parsed.tool_call.summary,
      })
      return 'handled'
    }
    if (typeof parsed.parsing_tool_calls === 'boolean') {
      callbacks.onParsingToolCallsChange?.(parsed.parsing_tool_calls)
      return 'handled'
    }
    if (typeof parsed.tool_running === 'boolean') {
      callbacks.onToolStatusChange?.(parsed.tool_running)
      return 'handled'
    }
    if (parsed.tool_result?.output != null) {
      callbacks.onToolResult?.({
        name: parsed.tool_result.name || '',
        summary: parsed.tool_result.summary,
        output: parsed.tool_result.output,
        ok: parsed.tool_result.ok,
        exit_code: parsed.tool_result.exit_code,
        error_code: parsed.tool_result.error_code,
        stdout: parsed.tool_result.stdout,
        stderr: parsed.tool_result.stderr,
      })
      return 'handled'
    }
    if (parsed.command_approval_request != null) {
      callbacks.onCommandApprovalRequest?.({
        command: parsed.command_approval_request.command || '',
        args: parsed.command_approval_request.args || '',
        allowlistKey: parsed.command_approval_request.allowlist_key || undefined,
      })
      return 'handled'
    }
    if (
      typeof parsed.staged_plan_notice === 'string' ||
      parsed.staged_plan_notice_clear === true
    ) {
      return 'handled'
    }
    if (typeof parsed.chat_ui_separator === 'boolean') {
      callbacks.onChatUiSeparator?.(parsed.chat_ui_separator)
      return 'handled'
    }
  } catch {
    // 非 JSON，按正文处理
  }
  return 'plain'
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
    /** 模型正在流式解析 tool_calls（选工具 / 拼参数） */
    onParsingToolCallsChange?: (parsing: boolean) => void
    onToolResult?: (info: ToolResultInfo) => void
    onCommandApprovalRequest?: (info: CommandApprovalRequestInfo) => void
    /** 预留：后端 `plan_required`（如 PER 结构化规划提示） */
    onPlanRequired?: () => void
    onStagedPlanStarted?: (info: { planId: string; totalSteps: number }) => void
    onStagedPlanStepStarted?: (info: {
      planId: string
      stepId: string
      stepIndex: number
      totalSteps: number
      description: string
    }) => void
    onStagedPlanStepFinished?: (info: {
      planId: string
      stepId: string
      stepIndex: number
      totalSteps: number
      status: string
    }) => void
    onStagedPlanFinished?: (info: {
      planId: string
      totalSteps: number
      completedSteps: number
      status: string
    }) => void
    onChatUiSeparator?: (short: boolean) => void
    /** 服务端返回会话 ID（首轮未传 conversation_id 时由后端生成） */
    onConversationId?: (id: string) => void
  },
  approvalSessionId?: string,
  signal?: AbortSignal,
  conversationId?: string,
): Promise<void> {
  const headers = new Headers({ 'Content-Type': 'application/json' })
  const bearerToken = getStoredWebApiBearerToken()
  if (bearerToken && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${bearerToken}`)
  }
  const r = await fetch(`${base}/chat/stream`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      message,
      conversation_id: conversationId || undefined,
      approval_session_id: approvalSessionId || undefined,
    }),
    signal,
  })
  if (!r.ok) {
    const data = await r.json().catch(() => ({}))
    callbacks.onError((data as { message?: string }).message || r.statusText)
    return
  }
  const serverConversationId = r.headers.get('x-conversation-id')?.trim() || ''
  if (serverConversationId) callbacks.onConversationId?.(serverConversationId)
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
        const d = tryDispatchSseControlPayload(data, callbacks)
        if (d === 'stop') return
        if (d === 'handled') continue
        if (data !== '[DONE]') callbacks.onDelta(data)
      }
    }
    if (buffer.trim()) {
      const dataLines = buffer.split('\n').filter((l) => l.startsWith('data: '))
      const data = dataLines.map((l) => l.slice(6).replace(/^\s+/, '')).join('\n').trim()
      if (data && data !== '[DONE]') {
        const d = tryDispatchSseControlPayload(data, callbacks)
        if (d === 'plain') callbacks.onDelta(data)
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

export async function submitChatApproval(
  approvalSessionId: string,
  decision: 'deny' | 'allow_once' | 'allow_always',
): Promise<{ ok: boolean }> {
  return request<{ ok: boolean }>('/chat/approval', {
    method: 'POST',
    timeoutMs: 15000,
    retries: 0,
    json: {
      approval_session_id: approvalSessionId,
      decision,
    },
  })
}
