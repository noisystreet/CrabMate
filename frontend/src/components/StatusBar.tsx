import { useState, useEffect } from 'react'
import { Cpu, Globe, Circle, Loader2, AlertCircle } from 'lucide-react'
import { fetchStatus } from '../api'
import type { StatusData } from '../types'

interface StatusBarProps {
  /** 大模型请求是否进行中 */
  busy?: boolean
  /** 工具（run_command/run_executable 等）是否有正在运行的任务 */
  toolBusy?: boolean
  error?: string | null
}

export function StatusBar({ busy = false, toolBusy = false, error = null }: StatusBarProps) {
  const [status, setStatus] = useState<StatusData | null>(null)
  const [statusError, setStatusError] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    let failCount = 0
    let timer: number | null = null
    const BASE_INTERVAL_MS = 10000
    const MAX_INTERVAL_MS = 60000

    const clearTimer = () => {
      if (timer != null) window.clearTimeout(timer)
      timer = null
    }

    const scheduleNext = (ms: number) => {
      clearTimer()
      if (!alive) return
      timer = window.setTimeout(() => {
        void poll()
      }, ms)
    }

    const calcNextInterval = () => {
      // 失败指数退避：10s * 2^failCount，上限 60s
      const next = Math.min(MAX_INTERVAL_MS, BASE_INTERVAL_MS * 2 ** Math.min(failCount, 3))
      return next
    }

    const poll = async () => {
      if (!alive) return
      // 页面不可见时暂停轮询，等恢复可见再继续
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
        scheduleNext(BASE_INTERVAL_MS)
        return
      }
      try {
        const s = await fetchStatus()
        if (!alive) return
        setStatus(s)
        setStatusError(null)
        failCount = 0
      } catch {
        if (!alive) return
        setStatusError('无法获取后台状态')
        failCount += 1
      } finally {
        scheduleNext(calcNextInterval())
      }
    }

    const onVisibilityChange = () => {
      if (!alive) return
      if (document.visibilityState === 'visible') {
        // 恢复可见时立刻刷新一次
        clearTimer()
        void poll()
      } else {
        // 不可见时停止当前计时器（彻底暂停）
        clearTimer()
      }
    }

    // SWR：fetchStatus 可能直接命中缓存并返回旧值，同时后台刷新更新缓存
    void poll()
    document.addEventListener('visibilitychange', onVisibilityChange)

    return () => {
      alive = false
      clearTimer()
      document.removeEventListener('visibilitychange', onVisibilityChange)
    }
  }, [])

  const msg = busy
    ? '模型生成中…'
    : toolBusy
      ? '工具运行中…'
      : error ?? (statusError ?? (status ? '就绪' : '加载中…'))

  return (
    <div className="border-t border-base-300 px-4 py-2.5 bg-base-200 text-sm overflow-x-auto rounded-none flex-shrink-0">
      <div className="flex flex-nowrap items-center gap-x-6 whitespace-nowrap min-w-max">
        <span className="flex items-center gap-2 text-base-content/70">
          <Cpu size={14} className="text-primary flex-shrink-0" />
          <span className="font-medium opacity-80">模型</span>
          <span>{status?.model ?? '—'}</span>
        </span>
        <span className="flex items-center gap-2 text-base-content/70">
          <Globe size={14} className="text-primary flex-shrink-0" />
          <span className="font-medium opacity-80">API</span>
          <span>{status?.api_base ?? '—'}</span>
        </span>
        <span
          className={
            busy || toolBusy
              ? 'flex items-center gap-2 text-warning'
              : error
                ? 'flex items-center gap-2 text-error'
                : 'flex items-center gap-2 text-success'
          }
        >
          <Circle size={14} className="opacity-60 flex-shrink-0" />
          <span className="font-medium opacity-80">状态</span>
          {(busy || toolBusy) && <Loader2 size={14} className="animate-spin flex-shrink-0" />}
          {error && <AlertCircle size={14} className="flex-shrink-0" />}
          <span>{msg}</span>
        </span>
      </div>
    </div>
  )
}
