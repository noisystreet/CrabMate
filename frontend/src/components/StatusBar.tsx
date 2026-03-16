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
    fetchStatus()
      .then(setStatus)
      .catch(() => setStatusError('无法获取后台状态'))
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
