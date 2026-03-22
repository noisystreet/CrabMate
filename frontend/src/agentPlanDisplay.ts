/**
 * 与后端 `agent::plan_artifact::format_agent_reply_plan_for_display` 展示语义一致：
 * 将可解析的 `agent_reply_plan` v1 转为简单 Markdown 有序列表（可选围栏前自然语言 + `1. \`id\`: description`），不展示原始 JSON。
 */

/** 与后端 `message_display::SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT` 对齐：规划轮不在主聊天区重复展示（侧栏/通知另有呈现）。 */
export const SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT = false

/** 与 `plan_section::STAGED_STEP_USER_BOILERPLATE` 一致（分步注入 user 长句）。 */
export const STAGED_STEP_USER_BOILERPLATE =
  '请只专注完成下列规划步骤，本步完成后以非 tool_calls 的终答结束；不要提前执行后续步骤。'

/** 与后端 `message_display::SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT` 对齐。 */
export const SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT = false

/** 与 `user_message_for_chat_display` / `is_staged_step_injection_user_content` 同形。 */
function isStagedStepInjectionUserContent(s: string): boolean {
  const t = s.trimStart()
  return t.startsWith('【分步执行') && t.includes('\n- id:') && t.includes('\n- 描述:')
}

/** 聊天区展示用：分步注入 `user` 在 `SHOW_…` 为 `false` 时整段不展示；`messages` 原文与导出不变。 */
export function formatStagedStepUserForChat(raw: string): string {
  if (SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT) return raw
  if (isStagedStepInjectionUserContent(raw)) return ''
  return raw
}

function proseBeforeFirstFence(content: string): string {
  const i = content.indexOf('```')
  if (i < 0) return ''
  return content.slice(0, i).trim()
}

function stripOptionalJsonFenceLabel(raw: string): string {
  const lines = raw.split('\n')
  if (lines.length === 0) return raw.trim()
  if (lines[0].trim().toLowerCase() === 'json') {
    return lines.slice(1).join('\n').trim()
  }
  return raw.trim()
}

function collectJsonCandidates(content: string): string[] {
  const out: string[] = []
  const parts = content.split('```')
  for (let i = 1; i < parts.length; i += 2) {
    const raw = parts[i].trim()
    if (!raw) continue
    const body = stripOptionalJsonFenceLabel(raw)
    if (body.startsWith('{')) out.push(body)
  }
  const all = content.trim()
  if (all.startsWith('{') && !out.some((s) => s === all)) out.push(all)
  return out
}

function isValidPlanV1(o: unknown): o is { steps: { id: string; description: string }[] } {
  if (typeof o !== 'object' || o === null) return false
  const p = o as { type?: unknown; version?: unknown; steps?: unknown }
  if (p.type !== 'agent_reply_plan' || p.version !== 1) return false
  if (!Array.isArray(p.steps) || p.steps.length === 0) return false
  for (const s of p.steps) {
    if (typeof s !== 'object' || s === null) return false
    const st = s as { id?: unknown; description?: unknown }
    if (typeof st.id !== 'string' || !st.id.trim()) return false
    if (typeof st.description !== 'string' || !st.description.trim()) return false
  }
  return true
}

/** 与后端 `plan_artifact::strip_agent_reply_plan_fence_blocks_for_display` 一致：去掉含 agent_reply_plan 的 ``` 块，避免聊天区打印原始 JSON。 */
export function stripAgentReplyPlanFenceBlocksForDisplay(content: string): string {
  const parts = content.split('```')
  let out = ''
  let i = 0
  while (i < parts.length) {
    out += parts[i]
    i += 1
    if (i >= parts.length) break
    const inner = parts[i]
    i += 1
    if (fenceInnerShouldHideAgentReplyPlanJson(inner)) {
      continue
    }
    out += '```' + inner + '```'
  }
  return out
}

function fenceInnerShouldHideAgentReplyPlanJson(inner: string): boolean {
  const body = stripOptionalJsonFenceLabel(inner)
  if (!body.startsWith('{')) return false
  try {
    const o = JSON.parse(body) as unknown
    if (isValidPlanV1(o)) return true
  } catch {
    /* fall through */
  }
  return body.includes('"agent_reply_plan"') && body.includes('"steps"')
}

/** 返回可读规划文本；无法解析则 `null`（由调用方继续展示原文/Markdown）。 */
export function tryFormatAgentReplyPlanForDisplay(content: string): string | null {
  let plan: { steps: { id: string; description: string }[] } | null = null
  for (const slice of collectJsonCandidates(content)) {
    try {
      const o = JSON.parse(slice) as unknown
      if (!isValidPlanV1(o)) continue
      plan = o
      break
    } catch {
      continue
    }
  }
  if (!plan) return null
  const goal = proseBeforeFirstFence(content).replace(/\s+/g, ' ').trim()
  let out = ''
  if (goal) {
    out += goal
    out += '\n\n'
  }
  plan.steps.forEach((st, i) => {
    const id = st.id.trim().replace(/`/g, "'")
    out += `${i + 1}. \`${id}\`: ${st.description.trim()}\n`
  })
  return out.trimEnd()
}
