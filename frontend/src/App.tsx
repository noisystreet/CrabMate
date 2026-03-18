import { useState, useRef, useEffect } from 'react'
import { ChatPanel } from './components/ChatPanel'
import { WorkspacePanel } from './components/WorkspacePanel'
import { TasksPanel } from './components/TasksPanel'
import { StatusBar } from './components/StatusBar'
import { ResizeHandle } from './components/ResizeHandle'
import { ThemeSwitcher } from './components/ThemeSwitcher'
import { deleteUploads, fetchTasks, saveTasks } from './api'
import type { TasksData } from './types'
import { ensureAtLeastOneSession, loadSessions, makeSessionId, saveSessions, type ChatSession, type StoredMessage } from './sessionStore'

const WORKSPACE_WIDTH_KEY = 'agent-demo-workspace-width'
const WORKSPACE_VISIBLE_KEY = 'agent-demo-workspace-visible'
const TASKS_VISIBLE_KEY = 'agent-demo-tasks-visible'
const STATUS_BAR_VISIBLE_KEY = 'agent-demo-status-bar-visible'
const DEFAULT_WORKSPACE_WIDTH = 280

function getStoredWorkspaceWidth(): number {
  if (typeof window === 'undefined') return DEFAULT_WORKSPACE_WIDTH
  const v = localStorage.getItem(WORKSPACE_WIDTH_KEY)
  const n = Number(v)
  return Number.isFinite(n) && n >= 200 && n <= 560 ? n : DEFAULT_WORKSPACE_WIDTH
}

function getStoredWorkspaceVisible(): boolean {
  if (typeof window === 'undefined') return true
  const v = localStorage.getItem(WORKSPACE_VISIBLE_KEY)
  if (v === '0' || v === 'false') return false
  return true
}

function getStoredTasksVisible(): boolean {
  if (typeof window === 'undefined') return false
  const v = localStorage.getItem(TASKS_VISIBLE_KEY)
  if (v === '0' || v === 'false') return false
  // 没有存储值时默认不显示任务清单
  if (v === null) return false
  return true
}

function getStoredStatusBarVisible(): boolean {
  if (typeof window === 'undefined') return true
  const v = localStorage.getItem(STATUS_BAR_VISIBLE_KEY)
  if (v === '0' || v === 'false') return false
  return true
}

export default function App() {
  const [statusBusy, setStatusBusy] = useState(false)
  const [statusError, setStatusError] = useState<string | null>(null)
  const [toolBusy, setToolBusy] = useState(false)
  const [workspaceWidth, setWorkspaceWidth] = useState(getStoredWorkspaceWidth)
  const [workspaceVisible, setWorkspaceVisible] = useState(getStoredWorkspaceVisible)
  const [tasksVisible, setTasksVisible] = useState(getStoredTasksVisible)
  const [statusBarVisible, setStatusBarVisible] = useState(getStoredStatusBarVisible)
  const [chatExportTrigger, setChatExportTrigger] = useState(0)
  const [chatHasMessages, setChatHasMessages] = useState(false)
  const [workspaceRefreshTrigger, setWorkspaceRefreshTrigger] = useState(0)
  const [externalSend, setExternalSend] = useState<{ seq: number; text: string } | null>(null)
  const [taskEvent, setTaskEvent] = useState<{ seq: number; text: string } | null>(null)
  const mainRowRef = useRef<HTMLDivElement>(null)

  const [{ sessions, activeId }, setSessionState] = useState<{
    sessions: ChatSession[]
    activeId: string
  }>(() => {
    const loaded = loadSessions()
    const ensured = ensureAtLeastOneSession(loaded.sessions)
    const chosen = loaded.activeId && ensured.sessions.some((s) => s.id === loaded.activeId) ? loaded.activeId : ensured.activeId
    saveSessions(ensured.sessions, chosen)
    return { sessions: ensured.sessions, activeId: chosen }
  })
  const [sessionModalOpen, setSessionModalOpen] = useState(false)
  const [sessionSearch, setSessionSearch] = useState('')
  const [sessionView, setSessionView] = useState<'all' | 'starred' | 'archived'>('all')
  const [selectedSessionIds, setSelectedSessionIds] = useState<Set<string>>(() => new Set())

  const activeSession = sessions.find((s) => s.id === activeId) ?? sessions[0]

  const autoTitleFromFirstUserMessage = (msgs: StoredMessage[]): string | null => {
    const first = msgs.find((m) => m.role === 'user' && (m.text || '').trim())
    if (!first) return null
    // 取首行，去掉“附件：”等尾部说明；并做长度截断
    const line = (first.text || '').split('\n')[0].trim()
    const cleaned = line.replace(/^附件[:：]\s*$/g, '').trim()
    if (!cleaned) return null
    const maxLen = 32
    return cleaned.length > maxLen ? cleaned.slice(0, maxLen) + '…' : cleaned
  }

  useEffect(() => {
    localStorage.setItem(WORKSPACE_WIDTH_KEY, String(workspaceWidth))
  }, [workspaceWidth])

  useEffect(() => {
    localStorage.setItem(WORKSPACE_VISIBLE_KEY, workspaceVisible ? '1' : '0')
  }, [workspaceVisible])

  useEffect(() => {
    localStorage.setItem(TASKS_VISIBLE_KEY, tasksVisible ? '1' : '0')
  }, [tasksVisible])

  useEffect(() => {
    localStorage.setItem(STATUS_BAR_VISIBLE_KEY, statusBarVisible ? '1' : '0')
  }, [statusBarVisible])

  useEffect(() => {
    saveSessions(sessions, activeId)
  }, [sessions, activeId])

  const updateSession = (id: string, patch: Partial<ChatSession>) => {
    setSessionState((st) => {
      const next = st.sessions.map((s) => (s.id === id ? { ...s, ...patch, updatedAt: Date.now() } : s))
      return { ...st, sessions: next }
    })
  }

  const setActiveSessionId = (id: string) => {
    setSessionState((st) => ({ ...st, activeId: id }))
  }

  const createSession = () => {
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
    setSessionState((st) => ({ sessions: [s, ...st.sessions], activeId: s.id }))
  }

  const deleteSession = (id: string) => {
    setSessionState((st) => {
      const next = st.sessions.filter((s) => s.id !== id)
      const ensured = ensureAtLeastOneSession(next)
      const nextActive = st.activeId === id ? ensured.activeId : st.activeId
      const finalActive = ensured.sessions.some((s) => s.id === nextActive) ? nextActive : ensured.activeId
      return { sessions: ensured.sessions, activeId: finalActive }
    })
  }

  const collectUploadUrlsFromSession = (s: ChatSession): string[] => {
    const urls: string[] = []
    const pushUrl = (u?: string) => {
      if (!u) return
      if (!u.startsWith('/uploads/')) return
      urls.push(u)
    }
    for (const m of s.messages || []) {
      for (const u of m.images || []) pushUrl(u)
      for (const u of m.audioUrls || []) pushUrl(u)
      for (const u of m.videoUrls || []) pushUrl(u)
      // 兼容文本里 “附件： - 图片：/uploads/..”
      const text = (m.text || '')
      const re = /\/uploads\/[A-Za-z0-9_.-]+/g
      const found = text.match(re) || []
      for (const f of found) pushUrl(f)
    }
    return Array.from(new Set(urls))
  }

  const exportAllSessions = () => {
    const data = { exportedAt: new Date().toISOString(), sessions }
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `chat_sessions_${Date.now()}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  const exportOneSession = (s: ChatSession) => {
    const data = { exportedAt: new Date().toISOString(), session: s }
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `chat_session_${s.id}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  const exportSessionMarkdown = (s: ChatSession) => {
    const lines: string[] = []
    lines.push(`# ${s.title || '未命名会话'}`)
    lines.push('')
    lines.push(`- id: \`${s.id}\``)
    lines.push(`- updatedAt: ${new Date(s.updatedAt).toISOString()}`)
    if (s.tags?.length) lines.push(`- tags: ${s.tags.join(', ')}`)
    if (s.starred) lines.push(`- starred: true`)
    if (s.archived) lines.push(`- archived: true`)
    lines.push('')
    lines.push('---')
    lines.push('')
    for (const m of s.messages || []) {
      const role = m.role === 'user' ? 'User' : m.role === 'assistant' ? 'Assistant' : 'System'
      lines.push(`## ${role}`)
      lines.push('')
      if (m.text) lines.push(m.text)
      const att: string[] = []
      for (const u of m.images || []) att.push(`- 图片：${u}`)
      for (const u of m.audioUrls || []) att.push(`- 音频：${u}`)
      for (const u of m.videoUrls || []) att.push(`- 视频：${u}`)
      if (att.length) {
        lines.push('')
        lines.push('附件：')
        lines.push(...att)
      }
      lines.push('')
      lines.push('---')
      lines.push('')
    }
    const content = lines.join('\n')
    const blob = new Blob([content], { type: 'text/markdown' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `chat_session_${s.id}.md`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  const exportSessionPdf = (s: ChatSession) => {
    // 简单方案：打开新窗口渲染纯文本/markdown 预格式化，然后调用 print（用户可“另存为 PDF”）
    const mdLines: string[] = []
    mdLines.push(`${s.title || '未命名会话'} (${new Date(s.updatedAt).toLocaleString()})`)
    mdLines.push('')
    for (const m of s.messages || []) {
      const role = m.role === 'user' ? 'User' : m.role === 'assistant' ? 'Assistant' : 'System'
      mdLines.push(`【${role}】`)
      if (m.text) mdLines.push(m.text)
      const att: string[] = []
      for (const u of m.images || []) att.push(`图片：${u}`)
      for (const u of m.audioUrls || []) att.push(`音频：${u}`)
      for (const u of m.videoUrls || []) att.push(`视频：${u}`)
      if (att.length) {
        mdLines.push('')
        mdLines.push(att.join('\n'))
      }
      mdLines.push('\n' + '-'.repeat(48) + '\n')
    }
    const printable = mdLines.join('\n')
    const w = window.open('', '_blank')
    if (!w) return
    w.document.open()
    w.document.write(`<!doctype html>
<html><head><meta charset="utf-8" />
<title>${(s.title || 'chat').replace(/</g, '&lt;')}</title>
<style>
  body{font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Helvetica, Arial; padding:24px;}
  pre{white-space: pre-wrap; word-break: break-word; font-size: 12px; line-height: 1.5;}
</style>
</head><body>
<pre>${printable.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</pre>
</body></html>`)
    w.document.close()
    w.focus()
    w.print()
  }

  const importSessionsFromFile = async (file: File) => {
    const text = await file.text()
    const parsed = JSON.parse(text) as any
    const incoming: ChatSession[] = Array.isArray(parsed?.sessions)
      ? (parsed.sessions as ChatSession[])
      : parsed?.session && typeof parsed.session === 'object'
        ? [parsed.session as ChatSession]
        : parsed && typeof parsed === 'object' && 'id' in parsed && 'messages' in parsed
          ? [parsed as ChatSession]
          : []
    if (!incoming.length) return
    setSessionState((st) => {
      const byId = new Map(st.sessions.map((s) => [s.id, s]))
      for (const s of incoming) {
        // 简单去重：同 id 覆盖（以导入为准）
        byId.set(s.id, s)
      }
      const merged = Array.from(byId.values()).sort((a, b) => b.updatedAt - a.updatedAt)
      const ensured = ensureAtLeastOneSession(merged)
      const active = ensured.sessions.some((x) => x.id === st.activeId) ? st.activeId : ensured.activeId
      return { sessions: ensured.sessions, activeId: active }
    })
  }

  return (
    <div className="h-screen flex flex-col overflow-hidden bg-base-100">
      {/* 顶部菜单栏 */}
      <div className="navbar min-h-0 h-12 shrink-0 bg-base-200/60 border-b border-base-300 px-3">
        <span className="text-lg font-semibold text-base-content shrink-0">CrabMate</span>
        <div className="flex-1" />
        <div className="flex gap-1">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => setSessionModalOpen(true)}
            title="会话管理"
          >
            会话
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => setChatExportTrigger((v) => v + 1)}
            disabled={!chatHasMessages}
            title={chatHasMessages ? '将当前会话保存为 JSON 文件' : '当前没有可保存的对话'}
          >
            保存会话
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm btn-square"
            onClick={() => setWorkspaceVisible((v) => !v)}
            title={workspaceVisible ? '隐藏工作区' : '显示工作区'}
          >
            {/* 侧边栏 / 文件列表面板：左侧主区域 + 右侧带列表的窄条 */}
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">
              <path d="M3 4v16h9V4H3z" />
              <path d="M14 4h7v16h-7z" />
              <path d="M15.5 8h2M15.5 11.5h2M15.5 15h1.5" />
            </svg>
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm btn-square"
            onClick={() => setTasksVisible((v) => !v)}
            title={tasksVisible ? '隐藏任务清单' : '显示任务清单'}
          >
            {/* 任务清单：列表图标 */}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              className="h-4 w-4"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={1.8}
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M8 6h13M8 12h13M8 18h13" />
              <path d="M3 6h.01M3 12h.01M3 18h.01" />
            </svg>
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm btn-square"
            onClick={() => setStatusBarVisible((v) => !v)}
            title={statusBarVisible ? '隐藏状态栏' : '显示状态栏'}
          >
            {/* 窗口 + 底部状态条：外框与底部分隔线，下方带指示小段 */}
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">
              <rect x="3" y="3" width="18" height="18" rx="1.5" />
              <path d="M3 16.5h18" />
              <path d="M6 19.2h1.5M11 19.2h1.5M16 19.2h1.5" />
            </svg>
          </button>
          <ThemeSwitcher />
        </div>
      </div>
      <div className="flex-1 min-h-0 flex flex-col">
        <div ref={mainRowRef} className="flex-1 min-h-0 flex overflow-hidden">
          <div className="flex-1 min-w-0 min-h-0 flex flex-col overflow-hidden">
            <ChatPanel
              onSendStart={() => {
                setStatusBusy(true)
                setStatusError(null)
              }}
              onSendEnd={(err) => {
                setStatusBusy(false)
                setStatusError(err ?? null)
              }}
              onWorkspaceChanged={() => setWorkspaceRefreshTrigger((t) => t + 1)}
              exportTrigger={chatExportTrigger}
              onMessagesChange={setChatHasMessages}
              onToolStatusChange={setToolBusy}
              externalSend={externalSend}
              systemInject={taskEvent}
              sessionId={activeId}
              initialMessages={(activeSession?.messages || []) as unknown as StoredMessage[] as any}
              initialDraft={activeSession?.draft || ''}
              onSessionSnapshot={({ messages, draft }) => {
                const msgs = messages as unknown as StoredMessage[]
                setSessionState((st) => {
                  const current = st.sessions.find((s) => s.id === st.activeId)
                  const shouldAutoTitle = current && (current.title === '新会话' || current.title === '未命名会话')
                  const maybeTitle = shouldAutoTitle ? autoTitleFromFirstUserMessage(msgs) : null
                  const next = st.sessions.map((s) => {
                    if (s.id !== st.activeId) return s
                    return {
                      ...s,
                      messages: msgs,
                      draft,
                      title: maybeTitle ?? s.title,
                      updatedAt: Date.now(),
                    }
                  })
                  return { ...st, sessions: next }
                })
              }}
              onAddTaskFromMessage={(title) => {
                const trimmed = title.trim()
                if (!trimmed) return
                ;(async () => {
                  try {
                    const current = await fetchTasks()
                    const base: TasksData = current ?? { items: [] }
                    const items = Array.isArray(base.items) ? base.items : []
                    const id = `${Date.now()}`
                    const next: TasksData = {
                      source: base.source ?? 'chat',
                      items: [...items, { id, title: trimmed, done: false }],
                    }
                    await saveTasks(next)
                  } catch (e) {
                    // eslint-disable-next-line no-console
                    console.error('从聊天创建任务失败', e)
                  }
                })()
              }}
            />
          </div>
          {(workspaceVisible || tasksVisible) && (
            <>
              <ResizeHandle containerRef={mainRowRef} onResize={setWorkspaceWidth} />
              <div className="flex min-h-0" style={{ width: workspaceWidth }}>
                {workspaceVisible && (
                  <WorkspacePanel
                    width={tasksVisible ? Math.max(180, workspaceWidth / 2) : workspaceWidth}
                    refreshTrigger={workspaceRefreshTrigger}
                    onSendToChat={(text) => setExternalSend({ seq: Date.now(), text })}
                  />
                )}
                {tasksVisible && (
                  <TasksPanel
                    width={workspaceVisible ? Math.max(180, workspaceWidth / 2) : workspaceWidth}
                    onTaskCompleted={(task, index) => {
                      setTaskEvent({
                        seq: Date.now(),
                        text: `任务已完成：#${index + 1} ${task.title}`,
                      })
                    }}
                  />
                )}
              </div>
            </>
          )}
        </div>
        {statusBarVisible && <StatusBar busy={statusBusy} toolBusy={toolBusy} error={statusError} />}
      </div>

      {sessionModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
          <div className="w-full max-w-[720px] bg-base-100 border border-base-300 rounded-none shadow-xl">
            <div className="flex items-center justify-between px-4 py-3 border-b border-base-300 bg-base-200">
              <div className="flex items-center gap-2">
                <span className="font-semibold">会话管理</span>
                <span className="text-xs text-base-content/60">（本地保存）</span>
              </div>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => setSessionModalOpen(false)}>
                关闭
              </button>
            </div>

            <div className="p-4 space-y-3">
              <div className="flex flex-wrap items-center gap-2">
                <div className="join">
                  <button type="button" className={`btn btn-sm rounded-none join-item ${sessionView === 'all' ? 'btn-active' : 'btn-ghost'}`} onClick={() => setSessionView('all')}>全部</button>
                  <button type="button" className={`btn btn-sm rounded-none join-item ${sessionView === 'starred' ? 'btn-active' : 'btn-ghost'}`} onClick={() => setSessionView('starred')}>收藏</button>
                  <button type="button" className={`btn btn-sm rounded-none join-item ${sessionView === 'archived' ? 'btn-active' : 'btn-ghost'}`} onClick={() => setSessionView('archived')}>归档</button>
                </div>
                <input
                  value={sessionSearch}
                  onChange={(e) => setSessionSearch(e.target.value)}
                  placeholder="搜索会话标题或内容…"
                  className="input input-bordered input-sm flex-1 min-w-[200px] rounded-none"
                />
                <button type="button" className="btn btn-primary btn-sm rounded-none" onClick={createSession}>
                  新建
                </button>
                <button type="button" className="btn btn-ghost btn-sm rounded-none" onClick={exportAllSessions}>
                  导出全部
                </button>
                <label className="btn btn-ghost btn-sm rounded-none">
                  导入（全部/单个）
                  <input
                    type="file"
                    accept="application/json"
                    className="hidden"
                    onChange={(e) => {
                      const f = e.target.files?.[0]
                      if (!f) return
                      void importSessionsFromFile(f)
                      e.target.value = ''
                    }}
                  />
                </label>
              </div>

              {selectedSessionIds.size > 0 && (
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="text-base-content/60">已选择 {selectedSessionIds.size} 个</span>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm rounded-none"
                    onClick={() => {
                      setSessionState((st) => ({
                        ...st,
                        sessions: st.sessions.map((s) => (selectedSessionIds.has(s.id) ? { ...s, archived: true, updatedAt: Date.now() } : s)),
                      }))
                      setSelectedSessionIds(new Set())
                    }}
                  >
                    批量归档
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm rounded-none"
                    onClick={() => {
                      setSessionState((st) => ({
                        ...st,
                        sessions: st.sessions.map((s) => (selectedSessionIds.has(s.id) ? { ...s, archived: false, updatedAt: Date.now() } : s)),
                      }))
                      setSelectedSessionIds(new Set())
                    }}
                  >
                    取消归档
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm rounded-none"
                    onClick={() => {
                      setSessionState((st) => ({
                        ...st,
                        sessions: st.sessions.map((s) => (selectedSessionIds.has(s.id) ? { ...s, starred: true, updatedAt: Date.now() } : s)),
                      }))
                      setSelectedSessionIds(new Set())
                    }}
                  >
                    批量收藏
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm rounded-none"
                    onClick={() => {
                      setSessionState((st) => ({
                        ...st,
                        sessions: st.sessions.map((s) => (selectedSessionIds.has(s.id) ? { ...s, starred: false, updatedAt: Date.now() } : s)),
                      }))
                      setSelectedSessionIds(new Set())
                    }}
                  >
                    取消收藏
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm rounded-none text-error"
                    onClick={() => {
                      if (!window.confirm('确定批量删除所选会话？此操作不可恢复（本地）。')) return
                      setSessionState((st) => {
                        const next = st.sessions.filter((s) => !selectedSessionIds.has(s.id))
                        const ensured = ensureAtLeastOneSession(next)
                        const active = ensured.sessions.some((x) => x.id === st.activeId) ? st.activeId : ensured.activeId
                        return { sessions: ensured.sessions, activeId: active }
                      })
                      setSelectedSessionIds(new Set())
                    }}
                  >
                    批量删除
                  </button>
                  <button type="button" className="btn btn-ghost btn-sm rounded-none" onClick={() => setSelectedSessionIds(new Set())}>
                    清空选择
                  </button>
                </div>
              )}

              <div className="max-h-[380px] overflow-auto border border-base-300">
                {sessions
                  .filter((s) => {
                    const q = sessionSearch.trim().toLowerCase()
                    if (!q) return true
                    if (s.title.toLowerCase().includes(q)) return true
                    return s.messages.some((m) => (m.text || '').toLowerCase().includes(q))
                  })
                  .filter((s) => {
                    if (sessionView === 'starred') return !!s.starred && !s.archived
                    if (sessionView === 'archived') return !!s.archived
                    return !s.archived
                  })
                  .map((s) => (
                    <div
                      key={s.id}
                      className={`flex items-center gap-2 px-3 py-2 border-b border-base-300 ${
                        s.id === activeId ? 'bg-base-200' : 'bg-base-100'
                      }`}
                    >
                      <input
                        type="checkbox"
                        className="checkbox checkbox-sm rounded-none"
                        checked={selectedSessionIds.has(s.id)}
                        onChange={(e) => {
                          setSelectedSessionIds((prev) => {
                            const next = new Set(prev)
                            if (e.target.checked) next.add(s.id)
                            else next.delete(s.id)
                            return next
                          })
                        }}
                        title="选择用于批量操作"
                      />
                      <button
                        type="button"
                        className="flex-1 text-left"
                        onClick={() => {
                          setActiveSessionId(s.id)
                          setSessionModalOpen(false)
                        }}
                        title="切换到该会话"
                      >
                        <div className="font-medium truncate">{s.title || '未命名会话'}</div>
                        <div className="text-xs text-base-content/60 flex gap-2">
                          <span>{new Date(s.updatedAt).toLocaleString()}</span>
                          <span>·</span>
                          <span>{s.messages.length} 条</span>
                          {s.starred && <span>· ⭐</span>}
                          {s.tags?.length ? <span className="truncate">· {s.tags.slice(0, 2).join(', ')}{s.tags.length > 2 ? '…' : ''}</span> : null}
                        </div>
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-xs rounded-none"
                        onClick={() => updateSession(s.id, { starred: !s.starred })}
                        title={s.starred ? '取消收藏' : '收藏'}
                      >
                        {s.starred ? '★' : '☆'}
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-xs rounded-none"
                        onClick={() => {
                          const raw = window.prompt('编辑标签（用逗号分隔）', (s.tags || []).join(','))
                          if (raw == null) return
                          const tags = raw.split(',').map((x) => x.trim()).filter(Boolean)
                          updateSession(s.id, { tags })
                        }}
                      >
                        标签
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-xs rounded-none"
                        onClick={() => updateSession(s.id, { archived: !s.archived })}
                        title={s.archived ? '取消归档' : '归档'}
                      >
                        {s.archived ? '取消归档' : '归档'}
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-xs rounded-none"
                        onClick={() => exportOneSession(s)}
                      >
                        导出
                      </button>
                      <button type="button" className="btn btn-ghost btn-xs rounded-none" onClick={() => exportSessionMarkdown(s)}>
                        导出MD
                      </button>
                      <button type="button" className="btn btn-ghost btn-xs rounded-none" onClick={() => exportSessionPdf(s)}>
                        导出PDF
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-xs rounded-none"
                        onClick={() => {
                          const title = window.prompt('重命名会话', s.title)
                          if (title == null) return
                          updateSession(s.id, { title: title.trim() || '未命名会话' })
                        }}
                      >
                        重命名
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-xs rounded-none text-error"
                        onClick={() => {
                          if (!window.confirm('确定删除该会话？此操作不可恢复（本地）。')) return
                          const also = window.confirm('同时删除该会话关联的上传文件（/uploads/*）吗？')
                          if (also) {
                            const urls = collectUploadUrlsFromSession(s)
                            if (urls.length) {
                              void deleteUploads(urls).catch(() => {})
                            }
                          }
                          deleteSession(s.id)
                        }}
                      >
                        删除
                      </button>
                    </div>
                  ))}
              </div>

              <div className="text-xs text-base-content/60">
                提示：会话与草稿会自动保存在浏览器本地存储中；“保存会话”按钮仍会导出当前会话单独 JSON。
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
