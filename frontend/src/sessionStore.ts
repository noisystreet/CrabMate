export type StoredMessage = {
  id: string
  role: 'user' | 'assistant' | 'system'
  text: string
  images?: string[]
  audioUrls?: string[]
  videoUrls?: string[]
  state?: 'loading' | 'error'
  collapsed?: boolean
  isToolOutput?: boolean
  /** Web 时间线节点（不入导出 JSON 的 OpenAI 形） */
  isTimelineMarker?: boolean
  timelineKind?: string
  timelineTitle?: string
  timelineDetail?: string
  errorKind?: 'network' | 'timeout' | 'server' | 'unknown'
  canRetry?: boolean
}

export type ChatSession = {
  id: string
  title: string
  tags?: string[]
  starred?: boolean
  archived?: boolean
  createdAt: number
  updatedAt: number
  draft: string
  messages: StoredMessage[]
}

const SESSIONS_KEY = 'agent-demo-sessions-v1'
const ACTIVE_ID_KEY = 'agent-demo-active-session-id'

export function makeSessionId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID()
  return `s_${Date.now()}_${Math.random().toString(16).slice(2)}`
}

export function loadSessions(): { sessions: ChatSession[]; activeId: string | null } {
  if (typeof window === 'undefined') return { sessions: [], activeId: null }
  const raw = localStorage.getItem(SESSIONS_KEY)
  const activeId = localStorage.getItem(ACTIVE_ID_KEY)
  if (!raw) return { sessions: [], activeId }
  try {
    const parsed = JSON.parse(raw) as { sessions?: ChatSession[] }
    const sessions = (Array.isArray(parsed.sessions) ? parsed.sessions : []).map((s) => ({
      ...s,
      tags: Array.isArray((s as any).tags) ? (s as any).tags : [],
      starred: Boolean((s as any).starred),
      archived: Boolean((s as any).archived),
    }))
    return { sessions, activeId }
  } catch {
    return { sessions: [], activeId }
  }
}

export function saveSessions(sessions: ChatSession[], activeId: string | null): void {
  if (typeof window === 'undefined') return
  localStorage.setItem(SESSIONS_KEY, JSON.stringify({ sessions }))
  if (activeId) localStorage.setItem(ACTIVE_ID_KEY, activeId)
  else localStorage.removeItem(ACTIVE_ID_KEY)
}

export function ensureAtLeastOneSession(existing: ChatSession[]): { sessions: ChatSession[]; activeId: string } {
  if (existing.length > 0) return { sessions: existing, activeId: existing[0].id }
  const now = Date.now()
  const s: ChatSession = {
    id: makeSessionId(),
    title: '新会话',
    tags: [],
    starred: false,
    archived: false,
    createdAt: now,
    updatedAt: now,
    draft: '',
    messages: [],
  }
  return { sessions: [s], activeId: s.id }
}

