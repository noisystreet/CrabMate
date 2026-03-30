import { useState, useEffect } from 'react'
import { Loader2, ListTodo, Plus, RefreshCw } from 'lucide-react'
import type { TasksData, TaskItem } from '../types'
import { fetchTasks, saveTasks, sendChat } from '../api'

interface TasksPanelProps {
  width?: number
  onTaskCompleted?: (task: TaskItem, index: number) => void
}

const EMPTY_TASKS: TasksData = { items: [] }

export function TasksPanel({ width = 280, onTaskCompleted }: TasksPanelProps) {
  const [data, setData] = useState<TasksData>(EMPTY_TASKS)
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [quickAddOpen, setQuickAddOpen] = useState(false)
  const [quickAddTitle, setQuickAddTitle] = useState('')
  const [generateOpen, setGenerateOpen] = useState(false)
  const [generateDesc, setGenerateDesc] = useState('')

  const load = () => {
    setLoading(true)
    setError(null)
    fetchTasks()
      .then((d) => setData(d ?? EMPTY_TASKS))
      .catch(() =>
        setError('加载任务清单失败：服务端暂不可用或请求失败。可稍后重试，或点击「从描述生成」新建清单。'),
      )
      .finally(() => setLoading(false))
  }

  useEffect(() => {
    load()
  }, [])

  const persist = async (next: TasksData) => {
    setSaving(true)
    setError(null)
    try {
      const saved = await saveTasks(next)
      setData(saved ?? next)
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存任务清单失败')
    } finally {
      setSaving(false)
    }
  }

  const toggleTask = (id: string) => {
    const items = data.items.map((t) => (t.id === id ? { ...t, done: !t.done } : t))
    // 找出本次被切换为 done 的任务，用于上报给外部（例如聊天系统消息）
    const idx = data.items.findIndex((t) => t.id === id)
    if (idx >= 0) {
      const prev = data.items[idx]
      const next = items[idx]
      if (!prev.done && next.done && onTaskCompleted) {
        onTaskCompleted(next, idx)
      }
    }
    persist({ ...data, items })
  }

  const openQuickAdd = () => {
    setQuickAddTitle('')
    setQuickAddOpen(true)
  }

  const submitQuickAdd = () => {
    const title = quickAddTitle.trim()
    if (!title) return
    const id = `${Date.now()}`
    const items: TaskItem[] = [...(data.items ?? []), { id, title, done: false }]
    persist({ ...data, items })
    setQuickAddOpen(false)
    setQuickAddTitle('')
  }

  const openGenerate = () => {
    setGenerateDesc('')
    setError(null)
    setGenerateOpen(true)
  }

  const submitGenerate = async () => {
    const desc = generateDesc.trim()
    if (!desc) return
    setSaving(true)
    setError(null)
    try {
      const prompt = `请根据下面的需求描述生成任务清单，只输出一个 JSON 对象，结构严格为：
{
  "source": string,        // 原始需求描述
  "items": [
    { "id": string, "title": string, "done": false }
  ]
}
不要输出任何额外解释或 Markdown，仅输出 JSON。

需求描述：
${desc}`

      const resp = await sendChat(prompt)
      let parsed: TasksData | null = null
      try {
        parsed = JSON.parse(resp.reply) as TasksData
      } catch {
        setError('模型返回内容无法解析为 JSON，请重试或手动编辑。')
        return
      }
      if (!parsed || !Array.isArray(parsed.items)) {
        setError('模型返回的任务结构不合法，请重试或手动编辑。')
        return
      }
      const normalized: TasksData = {
        source: parsed.source ?? desc,
        items: parsed.items.map((t, idx) => ({
          id: t.id || `${Date.now()}-${idx}`,
          title: t.title,
          done: !!t.done,
        })),
      }
      await persist(normalized)
      setGenerateOpen(false)
      setGenerateDesc('')
    } catch (e) {
      setError(e instanceof Error ? e.message : '生成任务清单失败')
    } finally {
      setSaving(false)
    }
  }

  const total = data.items?.length ?? 0
  const done = data.items?.filter((t) => t.done).length ?? 0
  const percent = total === 0 ? 0 : Math.round((done / total) * 100)
  let currentIndex = -1
  const current = data.items?.find((t, idx) => {
    if (!t.done && currentIndex === -1) {
      currentIndex = idx
      return true
    }
    return false
  }) || null

  return (
    <div
      className="card flex-shrink-0 flex flex-col h-full min-h-0 bg-base-100/85 backdrop-blur-sm border border-base-content/10 shadow-sm rounded-xl m-2 min-h-0"
      style={{ width: `${width}px` }}
    >
      <header className="flex-shrink-0 px-4 py-2 border-b border-base-content/10 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <ListTodo size={14} className="text-primary" />
          <h2 className="text-sm font-semibold text-base-content">任务清单</h2>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            className="btn btn-ghost btn-xs btn-square"
            title="刷新任务"
            onClick={load}
            disabled={loading || saving}
          >
            {loading ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-xs"
            title="根据描述自动生成任务清单"
            onClick={openGenerate}
            disabled={saving}
          >
            <Plus size={12} />
            <span>从描述生成</span>
          </button>
        </div>
      </header>
      <div className="flex-shrink-0 px-4 py-1 text-[11px] text-base-content/60 space-y-1">
        <div>
          {data.source
            ? `来源：${data.source.slice(0, 64)}${data.source.length > 64 ? '…' : ''}`
            : '尚未生成任务清单'}
        </div>
        {total > 0 && (
          <div className="flex flex-col gap-1">
            <div className="flex items-center justify-between">
              <span>进度：{done}/{total}（{percent}%）</span>
              {current && (
                <span className="truncate max-w-[160px]">
                  当前：#{currentIndex + 1} {current.title}
                </span>
              )}
            </div>
            <div className="w-full h-1.5 bg-base-300 relative overflow-hidden">
              <div
                className="h-1.5 bg-primary"
                style={{ width: `${percent}%`, transition: 'width 0.2s ease-out' }}
              />
              {percent > 0 && (
                <div
                  className="absolute top-1/2 -translate-y-1/2 w-2 h-2 rounded-full bg-primary border border-base-100"
                  style={{ left: `calc(${percent}% - 4px)` }}
                />
              )}
            </div>
          </div>
        )}
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {error && <p className="text-xs text-error mb-2">任务错误：{error}</p>}
        {loading && (
          <div className="flex items-center gap-2 text-base-content/60 py-4 px-2 text-sm">
            <Loader2 size={14} className="animate-spin" />
            <span>加载任务清单中…</span>
          </div>
        )}
        {!loading && (data.items?.length ?? 0) === 0 && (
          <div className="flex flex-col items-center justify-center text-base-content/60 text-sm py-6 px-2 gap-2">
            <ListTodo size={20} className="opacity-40" />
            <p className="text-center leading-relaxed">
              当前没有任务。你可以点击「从描述生成」自动拆解需求，或者使用「+」按钮快速新增一条任务。
            </p>
          </div>
        )}
        {!loading && data.items && data.items.length > 0 && (
          <ul className="space-y-1">
            {data.items.map((t) => (
              <li key={t.id} className="flex items-center gap-2 px-2 py-1 rounded-lg text-sm">
                <input
                  type="checkbox"
                  className="checkbox checkbox-xs rounded-lg"
                  checked={t.done}
                  onChange={() => toggleTask(t.id)}
                />
                <span className={`truncate ${t.done ? 'line-through text-base-content/40' : ''}`}>{t.title}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="flex-shrink-0 px-3 py-2 border-t border-base-300 flex items-center justify-between text-[11px] text-base-content/50">
        <button
          type="button"
          className="btn btn-ghost btn-xs rounded-lg"
          onClick={openQuickAdd}
          disabled={saving}
        >
          <Plus size={11} />
          快速添加
        </button>
        <span>
          共 {total} 条任务，已完成 {done}
        </span>
      </div>

      {/* 快速添加：本地面板输入 */}
      {quickAddOpen && (
        <>
          <div
            className="fixed inset-0 bg-black/50 z-50"
            aria-hidden
            onClick={() => { setQuickAddOpen(false); setQuickAddTitle(''); }}
          />
          <div className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[90vw] max-w-sm flex flex-col bg-base-200 border border-base-300 rounded-lg shadow-xl p-4 gap-3">
            <h3 className="text-sm font-semibold text-base-content">快速添加任务</h3>
            <input
              type="text"
              value={quickAddTitle}
              onChange={(e) => setQuickAddTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') submitQuickAdd()
                if (e.key === 'Escape') { setQuickAddOpen(false); setQuickAddTitle(''); }
              }}
              placeholder="输入任务描述"
              className="input input-bordered input-sm w-full rounded-lg"
              autoFocus
            />
            <div className="flex gap-2 justify-end">
              <button
                type="button"
                className="btn btn-ghost btn-sm rounded-lg"
                onClick={() => { setQuickAddOpen(false); setQuickAddTitle(''); }}
              >
                取消
              </button>
              <button
                type="button"
                className="btn btn-primary btn-sm rounded-lg"
                onClick={submitQuickAdd}
                disabled={!quickAddTitle.trim()}
              >
                确定
              </button>
            </div>
          </div>
        </>
      )}

      {/* 从描述生成：本地面板输入 */}
      {generateOpen && (
        <>
          <div
            className="fixed inset-0 bg-black/50 z-50"
            aria-hidden
            onClick={() => { if (!saving) { setGenerateOpen(false); setGenerateDesc(''); } }}
          />
          <div className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[90vw] max-w-lg max-h-[85vh] flex flex-col bg-base-200 border border-base-300 rounded-lg shadow-xl p-4 gap-3">
            <h3 className="text-sm font-semibold text-base-content shrink-0">从描述生成任务清单</h3>
            <p className="text-xs text-base-content/60 shrink-0">输入本次需求描述，将使用一次独立请求拆解为任务列表。</p>
            <textarea
              value={generateDesc}
              onChange={(e) => setGenerateDesc(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') { setGenerateOpen(false); setGenerateDesc(''); }
              }}
              placeholder="例如：为项目添加用户登录、注册与忘记密码功能"
              className="textarea textarea-bordered flex-1 min-h-[120px] w-full rounded-lg text-sm resize-y"
              spellCheck={false}
              autoFocus
            />
            <div className="flex gap-2 justify-end shrink-0">
              <button
                type="button"
                className="btn btn-ghost btn-sm rounded-lg"
                onClick={() => { setGenerateOpen(false); setGenerateDesc(''); }}
                disabled={saving}
              >
                取消
              </button>
              <button
                type="button"
                className="btn btn-primary btn-sm rounded-lg gap-1"
                onClick={() => submitGenerate()}
                disabled={!generateDesc.trim() || saving}
              >
                {saving ? (
                  <>
                    <Loader2 size={14} className="animate-spin" />
                    生成中…
                  </>
                ) : (
                  <span>确定</span>
                )}
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  )
}

