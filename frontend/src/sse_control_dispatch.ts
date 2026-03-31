/**
 * SSE `data:` 控制面 JSON 的分类与分发（无 DOM / fetch 依赖）。
 * 与 `src/sse/protocol.rs` 序列化形状及 `docs/SSE_PROTOCOL.md` 对齐；
 * 分支顺序须与 `src/sse/control_dispatch_mirror.rs` 中 `classify_sse_control_outcome` 一致。
 */

export type SseControlDispatch = 'stop' | 'handled' | 'plain'

export type SseControlPayload = {
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
    retryable?: boolean
    tool_call_id?: string
    execution_mode?: string
    parallel_batch_id?: string
    stdout?: string
    stderr?: string
  }
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
    status?: string
  }
  staged_plan_finished?: {
    plan_id?: string
    total_steps?: number
    completed_steps?: number
    status?: string
  }
  chat_ui_separator?: boolean
  command_approval_request?: {
    command?: string
    args?: string
    allowlist_key?: string
  }
  conversation_saved?: { revision?: number }
  timeline_log?: { kind?: string; title?: string; detail?: string }
}

/**
 * 已解析对象上的分类（与 Rust `control_dispatch_mirror` 同序）。
 * 不含 `JSON.parse`；非法结构由调用方保证为 object。
 */
export function classifySseControlPayloadParsed(parsed: SseControlPayload): SseControlDispatch {
  if (parsed.error != null) {
    const code =
      typeof parsed.code === 'string' && parsed.code.trim() !== '' ? parsed.code.trim() : null
    if (code != null) {
      return 'stop'
    }
  }
  if (parsed.plan_required === true) {
    return 'handled'
  }
  if (parsed.staged_plan_started != null) {
    return 'handled'
  }
  if (parsed.staged_plan_step_started != null) {
    return 'handled'
  }
  if (parsed.staged_plan_step_finished != null) {
    return 'handled'
  }
  if (parsed.staged_plan_finished != null) {
    return 'handled'
  }
  if (parsed.workspace_changed === true) {
    return 'handled'
  }
  if (parsed.tool_call?.summary) {
    return 'handled'
  }
  if (typeof parsed.parsing_tool_calls === 'boolean') {
    return 'handled'
  }
  if (typeof parsed.tool_running === 'boolean') {
    return 'handled'
  }
  if (parsed.tool_result?.output != null) {
    return 'handled'
  }
  if (parsed.command_approval_request != null) {
    return 'handled'
  }
  if (typeof parsed.staged_plan_notice === 'string' || parsed.staged_plan_notice_clear === true) {
    return 'handled'
  }
  if (typeof parsed.chat_ui_separator === 'boolean') {
    return 'handled'
  }
  if (parsed.conversation_saved != null) {
    return 'handled'
  }
  return 'plain'
}

export function classifySseControlPayload(data: string): SseControlDispatch {
  try {
    const parsed = JSON.parse(data) as SseControlPayload
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return 'plain'
    }
    return classifySseControlPayloadParsed(parsed)
  } catch {
    return 'plain'
  }
}

/** 与 `api.ts` 中 `ToolResultInfo` 同形，避免与 `api.ts` 循环引用 */
export type ToolResultInfoDispatch = {
  name: string
  summary?: string
  output: string
  ok?: boolean
  exit_code?: number
  error_code?: string
  retryable?: boolean
  tool_call_id?: string
  execution_mode?: string
  parallel_batch_id?: string
  stdout?: string
  stderr?: string
}

export type CommandApprovalRequestInfoDispatch = {
  command: string
  args: string
  allowlistKey?: string
}

export type SseControlCallbacks = {
  onError: (err: string) => void
  onWorkspaceChanged?: () => void
  onToolCall?: (info: { name: string; summary: string }) => void
  onToolStatusChange?: (running: boolean) => void
  onParsingToolCallsChange?: (parsing: boolean) => void
  onToolResult?: (info: ToolResultInfoDispatch) => void
  onCommandApprovalRequest?: (info: CommandApprovalRequestInfoDispatch) => void
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
}

/** 与历史 `api.ts` 中 `tryDispatchSseControlPayload` 行为一致。 */
export function tryDispatchSseControlPayload(
  data: string,
  callbacks: SseControlCallbacks,
): SseControlDispatch {
  try {
    const parsed = JSON.parse(data) as SseControlPayload
    if (parsed.error != null) {
      const code =
        typeof parsed.code === 'string' && parsed.code.trim() !== '' ? parsed.code.trim() : null
      if (code != null) {
        callbacks.onError(`${parsed.error} (${code})`)
        return 'stop'
      }
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
        retryable: parsed.tool_result.retryable,
        tool_call_id: parsed.tool_result.tool_call_id,
        execution_mode: parsed.tool_result.execution_mode,
        parallel_batch_id: parsed.tool_result.parallel_batch_id,
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
    if (parsed.conversation_saved != null) {
      return 'handled'
    }
  } catch {
    // 非 JSON，按正文处理
  }
  return 'plain'
}
