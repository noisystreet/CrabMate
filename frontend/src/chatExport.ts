/**
 * 与 Rust `chat_export` / `.crabmate/tui_session.json` 对齐的会话导出（JSON + Markdown）。
 * 版本号须与 `src/chat_export.rs` 中 `CHAT_SESSION_FILE_VERSION` 一致。
 */
import type { StoredMessage } from './sessionStore'

export const CRABMATE_CHAT_SESSION_FILE_VERSION = 1 as const

/** 与后端 `Message` JSON 形状兼容（仅导出用到的字段）。 */
export type CrabmateExportMessage = {
  role: string
  content: string | null
  tool_calls?: unknown
  name?: string | null
  tool_call_id?: string | null
}

export type CrabmateChatSessionFile = {
  version: typeof CRABMATE_CHAT_SESSION_FILE_VERSION
  messages: CrabmateExportMessage[]
}

export type ExportableChatMessage = {
  role: 'user' | 'assistant' | 'system'
  text: string
  images?: string[]
  audioUrls?: string[]
  videoUrls?: string[]
  isToolOutput?: boolean
}

/** 将 Web UI 消息转为可与 TUI `tui_session.json` 互导的 `messages` 项。 */
export function storedToCrabmateExportMessage(m: ExportableChatMessage): CrabmateExportMessage {
  const lines: string[] = []
  if (m.text?.trim()) lines.push(m.text)
  const att: string[] = []
  for (const u of m.images || []) att.push(`图片：${u}`)
  for (const u of m.audioUrls || []) att.push(`音频：${u}`)
  for (const u of m.videoUrls || []) att.push(`视频：${u}`)
  if (att.length) {
    if (lines.length) lines.push('')
    lines.push('附件：', ...att)
  }
  const content = lines.length > 0 ? lines.join('\n') : null
  if (m.role === 'system' && m.isToolOutput) {
    return { role: 'tool', content }
  }
  return { role: m.role, content }
}

export function buildCrabmateSessionFile(messages: ExportableChatMessage[]): CrabmateChatSessionFile {
  return {
    version: CRABMATE_CHAT_SESSION_FILE_VERSION,
    messages: messages.map(storedToCrabmateExportMessage),
  }
}

export function crabmateSessionFileToPrettyJson(file: CrabmateChatSessionFile): string {
  return JSON.stringify(file, null, 2)
}

/**
 * Markdown 正文与 Rust `chat_export::messages_to_markdown` 一致（默认标题）。
 * 若传入 `title`，则替换首行标题，并可加 `preamble`（如会话 id、标签）。
 */
export function buildCrabmateMarkdownFromMessages(
  messages: ExportableChatMessage[],
  opts?: { title?: string; preamble?: string[] },
): string {
  const title = opts?.title?.trim()
  let md = title ? `# ${title}\n\n` : '# CrabMate 聊天记录\n\n'
  if (opts?.preamble?.length) {
    for (const line of opts.preamble) md += `${line}\n`
    md += '\n---\n\n'
  }
  for (const m of messages) {
    if (m.role === 'system' && !m.isToolOutput) continue
    let heading: string
    if (m.role === 'system' && m.isToolOutput) heading = '## 工具'
    else if (m.role === 'user') heading = '## 用户'
    else if (m.role === 'assistant') heading = '## 助手'
    else continue
    md += `${heading}\n\n`
    const parts: string[] = []
    if (m.text) parts.push(m.text)
    for (const u of m.images || []) parts.push(`图片：${u}`)
    for (const u of m.audioUrls || []) parts.push(`音频：${u}`)
    for (const u of m.videoUrls || []) parts.push(`视频：${u}`)
    md += parts.join('\n')
    md += '\n\n'
  }
  return md
}

export function downloadBlob(filename: string, blob: Blob): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

/** 从 `StoredMessage[]` 导出与后端同形的 JSON 文件。 */
export function downloadCrabmateSessionJson(filename: string, messages: StoredMessage[]): void {
  const file = buildCrabmateSessionFile(messages)
  const blob = new Blob([crabmateSessionFileToPrettyJson(file)], { type: 'application/json' })
  downloadBlob(filename, blob)
}

export function downloadCrabmateSessionMarkdown(
  filename: string,
  messages: StoredMessage[],
  opts?: { title?: string; preamble?: string[] },
): void {
  const md = buildCrabmateMarkdownFromMessages(messages, opts)
  const blob = new Blob([md], { type: 'text/markdown;charset=utf-8' })
  downloadBlob(filename, blob)
}
