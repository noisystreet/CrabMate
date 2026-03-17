import { useState, useRef, useEffect } from 'react'
import { ChatPanel } from './components/ChatPanel'
import { WorkspacePanel } from './components/WorkspacePanel'
import { TasksPanel } from './components/TasksPanel'
import { StatusBar } from './components/StatusBar'
import { ResizeHandle } from './components/ResizeHandle'
import { ThemeSwitcher } from './components/ThemeSwitcher'
import { fetchTasks, saveTasks } from './api'
import type { TasksData } from './types'

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
    </div>
  )
}
