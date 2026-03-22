import { useState, useRef, useEffect, useCallback } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkMath from 'remark-math'
import remarkBreaks from 'remark-breaks'
import remarkGfm from 'remark-gfm'
import rehypeKatex from 'rehype-katex'
import 'katex/dist/katex.min.css'
import { Send, User, Bot, Loader2, ImagePlus, FileText, Mic, Video, Square } from 'lucide-react'
import type { Components } from 'react-markdown'
import { Virtuoso } from 'react-virtuoso'
import { tryFormatAgentReplyPlanForDisplay } from '../agentPlanDisplay'

const MAX_IMAGE_BYTES = 8 * 1024 * 1024
const MAX_AUDIO_BYTES = 25 * 1024 * 1024
const MAX_VIDEO_BYTES = 80 * 1024 * 1024
const IMAGE_MAX_W = 1600
const IMAGE_MAX_H = 1600
const IMAGE_QUALITY = 0.82

function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0B'
  const units = ['B', 'KB', 'MB', 'GB']
  let v = n
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i += 1
  }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)}${units[i]}`
}

async function compressImageToFile(file: File): Promise<File> {
  const bitmap = await createImageBitmap(file)
  const scale = Math.min(1, IMAGE_MAX_W / bitmap.width, IMAGE_MAX_H / bitmap.height)
  const w = Math.max(1, Math.round(bitmap.width * scale))
  const h = Math.max(1, Math.round(bitmap.height * scale))
  const canvas = document.createElement('canvas')
  canvas.width = w
  canvas.height = h
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('无法压缩图片')
  ctx.drawImage(bitmap, 0, 0, w, h)
  bitmap.close()
  const blob: Blob = await new Promise((resolve, reject) => {
    canvas.toBlob(
      (b) => {
        if (!b) reject(new Error('无法压缩图片'))
        else resolve(b)
      },
      'image/jpeg',
      IMAGE_QUALITY,
    )
  })
  const name = file.name.replace(/\.\w+$/, '') + '.jpg'
  return new File([blob], name, { type: 'image/jpeg' })
}

/** 将 [ ... LaTeX ... ] 转为 $$ ... $$，避免与 markdown 链接 [text](url) 冲突 */
function preprocessLatexBlocks(text: string): string {
  let s = text
  s = s.replace(/\[([^\]]*)\]/g, (match, inner, offset) => {
    if (!inner.includes('\\')) return match
    const nextChar = text[offset + match.length]
    if (nextChar === '(') return match
    return `$$${inner.trim()}$$`
  })
  // 单行或跨行的 $ ... $（内容含 \ 视为 LaTeX）转为 display 块，避免被当作文本
  s = s.replace(/\$[ \t]*([^$]*?)[ \t]*\n?[ \t]*\$/g, (match, inner) => {
    if (!inner.includes('\\')) return match
    return `$$\n${inner.trim()}\n$$`
  })
  // remark-math 要求 display math 的 $$ 单独成行，单行 $$...$$ 需改为多行块
  s = s.replace(/\$\$((?:(?!\$\$).)*?)\$\$/g, '$$\n$1\n$$')
  return s
}

/** 尝试为缺少换行的回答自动插入一些换行，提升可读性（尤其是中文长句和编号列表） */
function formatAssistantText(raw: string): string {
  const planText = tryFormatAgentReplyPlanForDisplay(raw)
  const source = planText ?? raw
  // 先做 LaTeX 预处理
  let s = preprocessLatexBlocks(source.replace(/\\n/g, '\n'))
  // 若本身已有换行，则直接返回（交给后端/模型自行控制段落），不再做任何空行压缩
  if (s.includes('\n')) {
    return s
  }
  // 在中文句号、问号、叹号后面尝试插入换行
  s = s.replace(/(。|！|？)/g, '$1\n')
  // 在编号列表前插入换行：1. 2. 3.，使用单个换行
  s = s.replace(/(\d+\.)(?=\S)/g, '\n$1 ')
  // 保持为单个换行，remark-breaks 会将其渲染为 <br>
  return s
}

/** 规范命令输出中的换行，避免前端展示时出现过多空行 */
function normalizeToolOutput(raw: string): string {
  return raw
    .replace(/\r\n/g, '\n')     // 统一换行符
    .replace(/\n{3,}/g, '\n\n') // 连续 3 行以上空行压缩为 1 个空行
    .replace(/\n+$/g, '')       // 去掉末尾多余空行
}

function langFromClassName(className?: string): string {
  if (!className) return ''
  const m = className.match(/language-([a-zA-Z0-9_-]+)/)
  return m?.[1] || ''
}

function downloadTextFile(name: string, content: string) {
  const blob = new Blob([content], { type: 'text/plain' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

function classifyErrorKind(msg: string): ErrorKind {
  const lower = msg.toLowerCase()
  if (lower.includes('timeout') || lower.includes('超时')) return 'timeout'
  if (lower.includes('network') || lower.includes('failed to fetch')) return 'network'
  if (lower.includes('internal_error') || lower.includes('对话失败')) return 'server'
  return 'unknown'
}
import { sendChatStream, uploadFiles } from '../api'
import { buildCrabmateSessionFile, crabmateSessionFileToPrettyJson, downloadBlob } from '../chatExport'

const INPUT_HEIGHT_KEY = 'agent-demo-input-height'
const MIN_INPUT_HEIGHT = 80
const MAX_INPUT_HEIGHT = 360
const DEFAULT_INPUT_HEIGHT = 120

function getStoredInputHeight(): number {
  if (typeof window === 'undefined') return DEFAULT_INPUT_HEIGHT
  const v = localStorage.getItem(INPUT_HEIGHT_KEY)
  const n = Number(v)
  return Number.isFinite(n) && n >= MIN_INPUT_HEIGHT && n <= MAX_INPUT_HEIGHT ? n : DEFAULT_INPUT_HEIGHT
}

type ErrorKind = 'network' | 'timeout' | 'server' | 'unknown'

type Message = {
  id: string
  role: 'user' | 'assistant' | 'system'
  text: string
  images?: string[]
  audioUrls?: string[]
  videoUrls?: string[]
  state?: 'loading' | 'error'
  collapsed?: boolean
  isToolOutput?: boolean
  errorKind?: ErrorKind
  canRetry?: boolean
}

type PendingAttachment = {
  id: string
  kind: 'image' | 'audio' | 'video'
  file: File
  previewUrl: string
  name: string
  size: number
  mime: string
}

interface ChatPanelProps {
  onSendStart?: () => void
  onSendEnd?: (error?: string) => void
  /** 当 Agent 通过工具创建/修改工作区文件时调用，用于刷新工作区列表 */
  onWorkspaceChanged?: () => void
  /** 当该值递增时导出当前会话（由父组件触发） */
  exportTrigger?: number
  /** 通知父组件当前是否有可保存的会话记录 */
  onMessagesChange?: (hasMessages: boolean) => void
  /** 会话切换时的 key（变化时会重置消息/草稿/附件/发送状态） */
  sessionId?: string
  /** 会话初始消息（用于切换会话时加载） */
  initialMessages?: Message[]
  /** 会话初始草稿 */
  initialDraft?: string
  /** 会话快照变化（用于父组件持久化） */
  onSessionSnapshot?: (snap: { messages: Message[]; draft: string }) => void
  /** 通知父组件工具运行状态（例如 run_command / run_executable 执行中） */
  onToolStatusChange?: (running: boolean) => void
  /** 来自工作区或其它面板的外部发送请求 */
  externalSend?: { seq: number; text: string } | null
  /** 外部注入的系统消息（例如任务完成进度），只追加到消息流，不触发发送 */
  systemInject?: { seq: number; text: string } | null
  /** 当需要从聊天消息中创建任务时回调（只传递 title 文本） */
  onAddTaskFromMessage?: (title: string) => void
}

export function ChatPanel({
  onSendStart,
  onSendEnd,
  onWorkspaceChanged,
  exportTrigger = 0,
  onMessagesChange,
  sessionId,
  initialMessages,
  initialDraft,
  onSessionSnapshot,
  onToolStatusChange,
  externalSend,
  systemInject,
  onAddTaskFromMessage,
}: ChatPanelProps) {
  const nextMsgSeqRef = useRef(1)
  const makeMessageId = useCallback(() => {
    if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID()
    const n = nextMsgSeqRef.current
    nextMsgSeqRef.current += 1
    return `m_${Date.now()}_${n}`
  }, [])

  const [input, setInput] = useState('')
  const [messages, setMessages] = useState<Message[]>([])
  const [pendingImages, setPendingImages] = useState<PendingAttachment[]>([])
  const [pendingAudios, setPendingAudios] = useState<PendingAttachment[]>([])
  const [pendingVideos, setPendingVideos] = useState<PendingAttachment[]>([])
  const [attachHint, setAttachHint] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const [, setPendingQueue] = useState<string[]>([])
  const inputRef = useRef('')
  const sendingRef = useRef(false)
  inputRef.current = input
  sendingRef.current = sending
  const [lastPrompt, setLastPrompt] = useState<string | null>(null)
  const [collapsedCodeBlocks, setCollapsedCodeBlocks] = useState<Record<string, boolean>>({})
  const [atBottom, setAtBottom] = useState(true)
  const [jumpOpen, setJumpOpen] = useState(false)
  const [jumpValue, setJumpValue] = useState('')
  const [uploading, setUploading] = useState(false)
  const [uploadPercent, setUploadPercent] = useState(0)
  const uploadAbortRef = useRef<AbortController | null>(null)
  const [inputHeight, setInputHeight] = useState(getStoredInputHeight)
  const listRef = useRef<HTMLDivElement>(null)
  const virtuosoRef = useRef<null | { scrollToIndex?: (arg: any) => void }>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const textFileInputRef = useRef<HTMLInputElement>(null)
  const audioInputRef = useRef<HTMLInputElement>(null)
  const videoInputRef = useRef<HTMLInputElement>(null)

  const isNearBottomRef = useRef(true)
  const scrollRafRef = useRef<number | null>(null)
  const scheduleScrollToBottom = useCallback(() => {
    if (!isNearBottomRef.current) return
    if (scrollRafRef.current != null) return
    scrollRafRef.current = window.requestAnimationFrame(() => {
      scrollRafRef.current = null
      const v = virtuosoRef.current
      if (v?.scrollToIndex) {
        v.scrollToIndex({ index: Math.max(0, messages.length - 1), align: 'end', behavior: 'auto' })
        return
      }
      const el = listRef.current
      if (!el) return
      el.scrollTo({ top: el.scrollHeight, behavior: 'auto' })
    })
  }, [messages.length])

  const handleAtBottomStateChange = useCallback((atBottom: boolean) => {
    isNearBottomRef.current = atBottom
    setAtBottom(atBottom)
  }, [])

  const scrollToIndex = useCallback((index: number) => {
    const v = virtuosoRef.current
    if (v?.scrollToIndex) {
      v.scrollToIndex({ index: Math.max(0, Math.min(messages.length - 1, index)), align: 'start', behavior: 'smooth' })
    }
  }, [messages.length])

  const scrollToBottomNow = useCallback(() => {
    const v = virtuosoRef.current
    if (v?.scrollToIndex) {
      v.scrollToIndex({ index: Math.max(0, messages.length - 1), align: 'end', behavior: 'smooth' })
    }
  }, [messages.length])

  // 流式输出：只更新“最后一条 assistant 文本节点”，避免频繁触发 React 重渲染
  const streamingMsgIdRef = useRef<string | null>(null)
  const streamingTextRef = useRef('')
  const streamingSpanRef = useRef<HTMLSpanElement | null>(null)
  const deltaBufferRef = useRef('')
  const deltaFlushRafRef = useRef<number | null>(null)

  const flushPendingDeltas = useCallback(() => {
    if (!deltaBufferRef.current) return
    const chunk = deltaBufferRef.current
    deltaBufferRef.current = ''
    streamingTextRef.current += chunk
    if (streamingSpanRef.current) {
      streamingSpanRef.current.textContent = streamingTextRef.current || '\u00A0'
    }
    scheduleScrollToBottom()
  }, [scheduleScrollToBottom])

  const enqueueDelta = useCallback((text: string) => {
    deltaBufferRef.current += text
    if (deltaFlushRafRef.current != null) return
    deltaFlushRafRef.current = window.requestAnimationFrame(() => {
      deltaFlushRafRef.current = null
      flushPendingDeltas()
    })
  }, [flushPendingDeltas])

  const markdownComponents: Components = {
    pre: ({ children }) => <>{children}</>,
    code: (props: any) => {
      const { inline, className, children } = props as { inline?: boolean; className?: string; children?: unknown }
      const raw = String(children ?? '')
      if (inline) return <code className={className}>{children as any}</code>
      const text = raw.replace(/\n$/, '')
      const lang = langFromClassName(className) || 'text'
      const key = `${lang}:${text.slice(0, 80)}:${text.length}`
      const collapsed = collapsedCodeBlocks[key] ?? (text.split('\n').length > 18)
      const shown = collapsed ? text.split('\n').slice(0, 18).join('\n') : text
      const fileName = `snippet.${lang === 'text' ? 'txt' : lang}`
      return (
        <div className="border border-base-300 bg-base-100 rounded-none overflow-hidden my-2">
          <div className="flex items-center justify-between px-2 py-1 bg-base-200 text-xs">
            <span className="font-mono text-base-content/70">{lang}</span>
            <div className="flex gap-1">
              <button
                type="button"
                className="btn btn-ghost btn-xs rounded-none"
                onClick={() => navigator.clipboard.writeText(text).catch(() => {})}
              >
                复制
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-xs rounded-none"
                onClick={() => downloadTextFile(fileName, text)}
              >
                下载
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-xs rounded-none"
                onClick={() => setCollapsedCodeBlocks((p) => ({ ...p, [key]: !collapsed }))}
              >
                {collapsed ? '展开' : '折叠'}
              </button>
            </div>
          </div>
          <pre className="m-0 p-3 text-[12px] leading-relaxed overflow-auto">
            <code className={className}>{shown}</code>
            {collapsed && text !== shown && <div className="pt-2 text-xs text-base-content/50">…已折叠</div>}
          </pre>
        </div>
      )
    },
  }

  useEffect(() => {
    return () => {
      if (scrollRafRef.current != null) window.cancelAnimationFrame(scrollRafRef.current)
      if (deltaFlushRafRef.current != null) window.cancelAnimationFrame(deltaFlushRafRef.current)
    }
  }, [])

  useEffect(() => {
    localStorage.setItem(INPUT_HEIGHT_KEY, String(inputHeight))
  }, [inputHeight])

  useEffect(() => {
    onMessagesChange?.(messages.length > 0)
  }, [messages, onMessagesChange])

  // 会话切换：重置 UI 状态并加载会话内容
  useEffect(() => {
    if (!sessionId) return
    // 取消当前请求（如果有）
    if (abortRef.current) {
      abortRef.current.abort()
      abortRef.current = null
    }
    // 清空流式缓存
    streamingMsgIdRef.current = null
    streamingSpanRef.current = null
    streamingTextRef.current = ''
    deltaBufferRef.current = ''
    // 重置状态
    sendingRef.current = false
    setSending(false)
    setPendingImages([])
    setPendingAudios([])
    setPendingVideos([])
    setAttachHint(null)
    setInput(initialDraft ?? '')
    setMessages(initialMessages ? [...initialMessages] : [])
    scheduleScrollToBottom()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId])

  // 将会话快照回传给父组件（做轻量节流，避免每次小变化都写 localStorage）
  useEffect(() => {
    if (!onSessionSnapshot) return
    const t = window.setTimeout(() => {
      onSessionSnapshot({ messages, draft: input })
    }, 300)
    return () => window.clearTimeout(t)
  }, [messages, input, onSessionSnapshot])

  const clearAttachments = useCallback(() => {
    setPendingImages((prev) => {
      prev.forEach((x) => URL.revokeObjectURL(x.previewUrl))
      return []
    })
    setPendingAudios((prev) => {
      prev.forEach((x) => URL.revokeObjectURL(x.previewUrl))
      return []
    })
    setPendingVideos((prev) => {
      prev.forEach((x) => URL.revokeObjectURL(x.previewUrl))
      return []
    })
    setAttachHint(null)
    setUploading(false)
    setUploadPercent(0)
    uploadAbortRef.current = null
  }, [])

  const addFiles = useCallback(async (files: FileList | File[]) => {
    const list = Array.from(files)
    if (!list.length) return
    setAttachHint(null)

    const imgs = list.filter((f) => f.type.startsWith('image/'))
    const audios = list.filter((f) => f.type.startsWith('audio/'))
    const videos = list.filter((f) => f.type.startsWith('video/'))

    const hints: string[] = []

    if (imgs.length) {
      const okImgs = imgs.filter((f) => {
        if (f.size > MAX_IMAGE_BYTES) {
          hints.push(`图片过大已跳过：${f.name}（${formatBytes(f.size)}，上限 ${formatBytes(MAX_IMAGE_BYTES)}）`)
          return false
        }
        return true
      })
      const items = await Promise.all(
        okImgs.map(async (f) => {
          let file = f
          try {
            if (f.size > 600 * 1024) file = await compressImageToFile(f)
          } catch {
            // ignore
          }
          const previewUrl = URL.createObjectURL(file)
          return {
            id: makeMessageId(),
            kind: 'image' as const,
            file,
            previewUrl,
            name: file.name,
            size: file.size,
            mime: file.type || 'application/octet-stream',
          }
        }),
      )
      if (items.length) setPendingImages((prev) => [...prev, ...items])
      if (okImgs.length) hints.push(`已添加 ${okImgs.length} 张图片（大图会自动压缩）`)
    }

    if (audios.length) {
      const okAudios = audios.filter((f) => {
        if (f.size > MAX_AUDIO_BYTES) {
          hints.push(`音频过大已跳过：${f.name}（${formatBytes(f.size)}，上限 ${formatBytes(MAX_AUDIO_BYTES)}）`)
          return false
        }
        return true
      })
      if (okAudios.length) hints.push(`提示：音频将以 multipart 上传后引用，不会塞进内存（上限 ${formatBytes(MAX_AUDIO_BYTES)}）`)
      const items = okAudios.map((f) => ({
        id: makeMessageId(),
        kind: 'audio' as const,
        file: f,
        previewUrl: URL.createObjectURL(f),
        name: f.name,
        size: f.size,
        mime: f.type || 'application/octet-stream',
      }))
      if (items.length) setPendingAudios((prev) => [...prev, ...items])
    }

    if (videos.length) {
      const okVideos = videos.filter((f) => {
        if (f.size > MAX_VIDEO_BYTES) {
          hints.push(`视频过大已跳过：${f.name}（${formatBytes(f.size)}，上限 ${formatBytes(MAX_VIDEO_BYTES)}）`)
          return false
        }
        return true
      })
      if (okVideos.length) hints.push(`提示：视频将以 multipart 上传后引用，不会塞进内存（上限 ${formatBytes(MAX_VIDEO_BYTES)}）`)
      const items = okVideos.map((f) => ({
        id: makeMessageId(),
        kind: 'video' as const,
        file: f,
        previewUrl: URL.createObjectURL(f),
        name: f.name,
        size: f.size,
        mime: f.type || 'application/octet-stream',
      }))
      if (items.length) setPendingVideos((prev) => [...prev, ...items])
    }

    if (hints.length) setAttachHint(hints.join('；'))
  }, [makeMessageId])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const files = e.dataTransfer.files
    if (!files?.length) return
    void addFiles(files)
  }, [addFiles])

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
  }, [])

  // 外部请求（例如 WorkspacePanel 中“将结果发送到聊天”）
  useEffect(() => {
    if (!externalSend || !externalSend.text.trim()) return
    const text = externalSend.text.trim()
    // 若当前正在发送，则加入发送队列
    if (sendingRef.current) {
      setPendingQueue((q) => [...q, text])
      return
    }
    // 立即发起一轮发送（忽略返回值）
    void send(text)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [externalSend?.seq])

  // 外部系统消息注入（例如任务完成进度），只追加一条 system 消息
  useEffect(() => {
    if (!systemInject || !systemInject.text.trim()) return
    const text = systemInject.text.trim()
    setMessages((m) => [...m, { id: makeMessageId(), role: 'system', text }])
    scheduleScrollToBottom()
  }, [systemInject?.seq])

  useEffect(() => {
    if (exportTrigger > 0) {
      exportConversation()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [exportTrigger])

  const exportConversation = () => {
    if (messages.length === 0) return
    const file = buildCrabmateSessionFile(messages)
    const blob = new Blob([crabmateSessionFileToPrettyJson(file)], { type: 'application/json' })
    const date = new Date()
    const ts = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(
      date.getDate(),
    ).padStart(2, '0')}_${String(date.getHours()).padStart(2, '0')}${String(
      date.getMinutes(),
    ).padStart(2, '0')}`
    downloadBlob(`chat_session_${ts}.json`, blob)
  }

  const handleInputResizeMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    if (!panelRef.current) return
    const onMouseMove = (moveEvent: MouseEvent) => {
      if (!panelRef.current) return
      const rect = panelRef.current.getBoundingClientRect()
      const heightFromBottom = rect.bottom - moveEvent.clientY
      const next = Math.min(MAX_INPUT_HEIGHT, Math.max(MIN_INPUT_HEIGHT, heightFromBottom))
      setInputHeight(next)
    }
    const onMouseUp = () => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseup', onMouseUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'row-resize'
    document.body.style.userSelect = 'none'
    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup', onMouseUp)
  }, [])

  const handleInputResizePointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault()
    if (!panelRef.current) return
    // 捕获指针，避免拖动时丢事件（触控板/触屏/鼠标均适用）
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      // ignore
    }

    const onPointerMove = (moveEvent: PointerEvent) => {
      if (!panelRef.current) return
      const rect = panelRef.current.getBoundingClientRect()
      const heightFromBottom = rect.bottom - moveEvent.clientY
      const next = Math.min(MAX_INPUT_HEIGHT, Math.max(MIN_INPUT_HEIGHT, heightFromBottom))
      setInputHeight(next)
    }
    const onPointerUp = () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'row-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
  }, [])

  const send = async (overrideInput?: string) => {
    const msg = (overrideInput ?? inputRef.current).trim()
    const hasAny = pendingImages.length > 0 || pendingAudios.length > 0 || pendingVideos.length > 0
    let images: string[] | undefined
    let audioUrls: string[] | undefined
    let videoUrls: string[] | undefined
    if (!msg && !hasAny) return
    // 若当前正在发送且本次不含附件，则将文本加入队列，等待上一轮结束后自动发送
    if (sendingRef.current && !hasAny) {
      if (msg) {
        setPendingQueue((q) => [...q, msg])
      }
      if (overrideInput === undefined) {
        setInput('')
      }
      return
    }
    setInput('')

    // 若有附件：先 multipart 上传，拿到 /uploads/... URL 再发起聊天
    if (hasAny) {
      try {
        setUploading(true)
        setUploadPercent(0)
        setAttachHint('附件上传中…')
        const ac = new AbortController()
        uploadAbortRef.current = ac
        const files = [...pendingImages, ...pendingAudios, ...pendingVideos].map((x) => x.file)
        const res = await uploadFiles(files, {
          signal: ac.signal,
          onProgress: (p) => {
            setUploadPercent(p.percent)
          },
        })
        const urls = res.files.map((f) => f.url)
        const imgCount = pendingImages.length
        const audioCount = pendingAudios.length
        const videoCount = pendingVideos.length
        images = imgCount ? urls.slice(0, imgCount) : undefined
        audioUrls = audioCount ? urls.slice(imgCount, imgCount + audioCount) : undefined
        videoUrls = videoCount ? urls.slice(imgCount + audioCount) : undefined
      } catch (e) {
        const err = e instanceof Error ? e.message : '附件上传失败'
        setAttachHint(err)
        return
      } finally {
        setUploading(false)
        clearAttachments()
      }
    }

    const fallback = images ? '(图片)' : audioUrls ? '(音频)' : videoUrls ? '(视频)' : ''
    setLastPrompt(msg)
    const attachmentLines: string[] = []
    if (images?.length) attachmentLines.push(...images.map((u) => `- 图片：${u}`))
    if (audioUrls?.length) attachmentLines.push(...audioUrls.map((u) => `- 音频：${u}`))
    if (videoUrls?.length) attachmentLines.push(...videoUrls.map((u) => `- 视频：${u}`))
    const fullMsg = attachmentLines.length ? `${msg || fallback}\n\n附件：\n${attachmentLines.join('\n')}` : (msg || fallback)
    const userId = makeMessageId()
    const assistantId = makeMessageId()
    streamingMsgIdRef.current = assistantId
    streamingTextRef.current = ''
    deltaBufferRef.current = ''
    if (streamingSpanRef.current) streamingSpanRef.current.textContent = '\u00A0'
    setMessages((m) => [
      ...m,
      { id: userId, role: 'user', text: fullMsg, images, audioUrls, videoUrls },
      { id: assistantId, role: 'assistant', text: '', state: 'loading' },
    ])
    scheduleScrollToBottom()
    sendingRef.current = true
    setSending(true)
    onSendStart?.()
    let finished = false
    try {
      if (abortRef.current) {
        abortRef.current.abort()
      }
      const controller = new AbortController()
      abortRef.current = controller
      await sendChatStream(fullMsg, {
        onDelta: (text) => {
          enqueueDelta(text)
        },
        onWorkspaceChanged,
        onToolCall: ({ summary }) => {
          setMessages((m) => [
            ...m,
            {
              id: makeMessageId(),
              role: 'system',
              text: summary,
              collapsed: true,
            },
          ])
          scheduleScrollToBottom()
        },
        onToolStatusChange,
        onToolResult: ({ name, output, ok, exit_code, error_code }) => {
          // 将工具输出也插入到对话中，使用专门的 system 样式，便于查看 ls 等命令结果
          const header = name ? `命令输出（${name}）` : '命令输出'
          const statusParts: string[] = []
          if (typeof ok === 'boolean') statusParts.push(ok ? '成功' : '失败')
          if (typeof exit_code === 'number') statusParts.push(`exit=${exit_code}`)
          if (error_code) statusParts.push(`code=${error_code}`)
          const statusLine = statusParts.length ? `状态：${statusParts.join(' | ')}` : ''
          const body = normalizeToolOutput(output)
          setMessages((m) => [
            ...m,
            {
              id: makeMessageId(),
              role: 'system',
              text: statusLine ? `${header}\n${statusLine}\n${body}` : `${header}\n${body}`,
              collapsed: true,
              isToolOutput: true,
            },
          ])
          scheduleScrollToBottom()
        },
        onDone: () => {
          finished = true
          flushPendingDeltas()
          setMessages((m) => {
            const next = [...m]
            let idx = -1
            for (let j = next.length - 1; j >= 0; j -= 1) {
              if (next[j].role === 'assistant' && next[j].state === 'loading') {
                idx = j
                break
              }
            }
            if (idx >= 0) {
              const t = streamingTextRef.current.trim()
              next[idx] = { ...next[idx], role: 'assistant', text: t || '(无回复)', state: undefined }
            }
            return next
          })
          streamingMsgIdRef.current = null
          streamingSpanRef.current = null
          streamingTextRef.current = ''
          onSendEnd?.()
          sendingRef.current = false
          setSending(false)
          // 若有排队的下一条消息，则在本轮结束后自动发送
          setPendingQueue((q) => {
            if (!q.length) return q
            const [next, ...rest] = q
            // 异步触发下一轮发送，避免与当前状态更新冲突
            setTimeout(() => {
              void send(next)
            }, 0)
            return rest
          })
        },
        onError: (errMsg) => {
          finished = true
          flushPendingDeltas()
          setMessages((m) => {
            const next = [...m]
            let idx = -1
            for (let j = next.length - 1; j >= 0; j -= 1) {
              if (next[j].role === 'assistant' && next[j].state === 'loading') {
                idx = j
                break
              }
            }
            if (idx >= 0) {
              const kind = classifyErrorKind(errMsg)
              next[idx] = { ...next[idx], role: 'assistant', text: errMsg, state: 'error', errorKind: kind, canRetry: !!lastPrompt }
            }
            return next
          })
          streamingMsgIdRef.current = null
          streamingSpanRef.current = null
          streamingTextRef.current = ''
          onSendEnd?.(errMsg)
          sendingRef.current = false
          setSending(false)
          // 错误场景同样尝试发送队列中的下一条
          setPendingQueue((q) => {
            if (!q.length) return q
            const [next, ...rest] = q
            setTimeout(() => {
              void send(next)
            }, 0)
            return rest
          })
        },
      }, controller.signal)
    } catch (e) {
      const msgText = e instanceof Error ? e.message : '请求失败'
      flushPendingDeltas()
      setMessages((m) => {
        const next = [...m]
        let idx = -1
        for (let j = next.length - 1; j >= 0; j -= 1) {
          if (next[j].role === 'assistant' && next[j].state === 'loading') {
            idx = j
            break
          }
        }
        if (idx >= 0) {
          const kind = classifyErrorKind(msgText)
          next[idx] = { ...next[idx], role: 'assistant', text: msgText, state: 'error', errorKind: kind, canRetry: !!lastPrompt }
        }
        return next
      })
      streamingMsgIdRef.current = null
      streamingSpanRef.current = null
      streamingTextRef.current = ''
      finished = true
      onSendEnd?.(msgText)
      sendingRef.current = false
      setSending(false)
      // 尝试发送队列中的下一条
      setPendingQueue((q) => {
        if (!q.length) return q
        const [next, ...rest] = q
        setTimeout(() => {
          void send(next)
        }, 0)
        return rest
      })
    } finally {
      // 兜底：若 sendChatStream 正常返回但未触发 onDone/onError（例如某些异常路径），
      // 也要确保结束本轮忙碌状态，避免状态栏一直显示“生成中”
      if (!finished) {
        onSendEnd?.()
        sendingRef.current = false
        setSending(false)
      }
    }
  }

  const cancel = () => {
    if (!sendingRef.current) return
    if (abortRef.current) {
      abortRef.current.abort()
      abortRef.current = null
    }
    flushPendingDeltas()
    sendingRef.current = false
    setSending(false)
    setMessages((m) => {
      const next = [...m]
      let idx = -1
      for (let j = next.length - 1; j >= 0; j -= 1) {
        if (next[j].role === 'assistant' && next[j].state === 'loading') {
          idx = j
          break
        }
      }
      if (idx >= 0) {
        const t = streamingTextRef.current.trim()
        next[idx] = {
          ...next[idx],
          role: 'assistant',
          text: (t && `${t}\n\n(本轮回答已中止)`) || '(本轮回答已中止)',
          state: 'error',
        }
      }
      return next
    })
    streamingMsgIdRef.current = null
    streamingSpanRef.current = null
    streamingTextRef.current = ''
    onSendEnd?.('已中止')
  }

  return (
    <div
      ref={panelRef}
      className="card flex flex-col h-full min-h-0 bg-base-200 border border-base-300 border-r-0 border-b-0 shadow-none rounded-none"
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      title="可将图片/音频/视频文件拖拽到此处上传"
    >
      <div ref={listRef} className="flex-1 min-h-0 overflow-hidden">
        {messages.length === 0 ? (
          <div className="h-full overflow-y-auto p-4">
            <div className="flex flex-col items-center justify-center text-base-content/60 text-sm py-8 gap-3">
              <Bot size={24} className="opacity-40" />
              <p className="text-center">输入消息并发送，开始对话</p>
            </div>
          </div>
        ) : (
          <div className="relative h-full">
            <Virtuoso
              ref={virtuosoRef as any}
              className="h-full"
              data={messages}
              atBottomStateChange={handleAtBottomStateChange}
              followOutput={isNearBottomRef.current ? 'smooth' : false}
              itemContent={(_idx, m) => (
              m.role === 'system' && m.isToolOutput ? (
            // 命令输出：使用卡片样式，可折叠 + 复制
            <div key={m.id} className="flex justify-center text-xs text-base-content/70 px-4 pt-4">
              <div className="w-full max-w-[720px] border border-base-300 bg-base-200 rounded-md overflow-hidden">
                {(() => {
                  const idx = m.text.indexOf('\n')
                  const title = idx >= 0 ? m.text.slice(0, idx) : m.text
                  const body = idx >= 0 ? m.text.slice(idx + 1) : ''
                  return (
                    <>
                      <div className="flex items-center justify-between px-3 py-1.5 bg-base-300">
                        <span className="truncate">{title}</span>
                        <div className="flex gap-1">
                          <button
                            type="button"
                            className="btn btn-ghost btn-xs"
                            onClick={() =>
                              setMessages((prev) => {
                                const id = m.id
                                return prev.map((x) => (x.id === id ? { ...x, collapsed: !x.collapsed } : x))
                              })
                            }
                          >
                            {m.collapsed ? '展开' : '折叠'}
                          </button>
                          <button
                            type="button"
                            className="btn btn-ghost btn-xs"
                            onClick={() => navigator.clipboard.writeText(body || title)}
                          >
                            复制
                          </button>
                        </div>
                      </div>
                      {!m.collapsed && body && (
                        <pre className="max-h-56 overflow-auto px-3 py-2 text-[11px] font-mono whitespace-pre-wrap leading-relaxed bg-base-100">
                          {body}
                        </pre>
                      )}
                    </>
                  )
                })()}
              </div>
            </div>
          ) : m.role === 'system' ? (
            // 其他 system 消息（如工具调用摘要）：保持 pill 样式
            <div key={m.id} className="flex justify-center text-xs text-base-content/60 px-4 pt-4">
              <button
                type="button"
                className="inline-flex items-center gap-1 px-2 py-1 rounded-full bg-base-300/80 hover:bg-base-300 transition-colors"
                onClick={() =>
                  setMessages((prev) => {
                    const id = m.id
                    return prev.map((x) => (x.id === id ? { ...x, collapsed: !x.collapsed } : x))
                  })
                }
              >
                <span className="font-mono truncate max-w-[220px]">
                  {m.collapsed ? m.text : `🔧 ${m.text}`}
                </span>
                <span className="text-[9px]">{m.collapsed ? '展开' : '收起'}</span>
              </button>
            </div>
          ) : (
            <div
              key={m.id}
              className={`flex gap-3 items-end px-4 pt-4 ${m.role === 'user' ? 'flex-row-reverse' : ''}`}
              onContextMenu={(e) => {
                e.preventDefault()
                if (!m.text) return
                // 右键菜单：优先复制整条；若按住 Shift 右键，则作为任务添加
                if (e.shiftKey && onAddTaskFromMessage) {
                  onAddTaskFromMessage(m.text)
                } else {
                  navigator.clipboard.writeText(m.text).catch(() => {})
                }
              }}
              title={
                m.role === 'assistant'
                  ? '右键复制整条，Shift+右键可将该条添加到任务清单'
                  : '右键复制整条'
              }
            >
              <div
                className={`flex-shrink-0 w-10 h-10 flex items-center justify-center rounded-none ${
                  m.role === 'user' ? 'bg-success text-success-content' : 'bg-primary text-primary-content'
                }`}
              >
                {m.role === 'user' ? <User size={20} /> : <Bot size={20} />}
              </div>
              <div
                className={`max-w-[78%] px-4 py-2.5 rounded-none ${
                  m.role === 'user'
                    ? 'bg-primary text-primary-content'
                    : m.state === 'error'
                      ? 'bg-error/20 text-error border border-error/30'
                      : 'bg-base-300 text-base-content border border-base-content/10'
                }`}
              >
                {m.state === 'loading' ? (
                  <div className="flex items-start gap-2 text-base-content/60">
                    <Loader2 size={16} className="animate-spin flex-shrink-0 mt-0.5" />
                    <span
                      ref={(el) => {
                        if (!el) return
                        if (m.id === streamingMsgIdRef.current) {
                          streamingSpanRef.current = el
                          el.textContent = streamingTextRef.current || '\u00A0'
                        }
                      }}
                      className="whitespace-pre-wrap break-words"
                    >
                      {m.text || '\u00A0'}
                    </span>
                  </div>
                ) : m.role === 'assistant' && !m.state ? (
                  <div className="markdown-body text-[15px] leading-relaxed">
                    <ReactMarkdown
                      remarkPlugins={[remarkMath, remarkBreaks, remarkGfm]}
                      rehypePlugins={[rehypeKatex]}
                      components={markdownComponents}
                    >
                      {formatAssistantText(m.text)}
                    </ReactMarkdown>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {m.images && m.images.length > 0 && (
                      <div className="flex flex-wrap gap-2">
                        {m.images.map((src, j) => (
                          <img
                            key={j}
                            src={src}
                            alt=""
                            className="max-w-[200px] max-h-[180px] w-auto h-auto object-contain rounded-lg border border-base-content/10 bg-base-100"
                          />
                        ))}
                      </div>
                    )}
                    {m.audioUrls && m.audioUrls.length > 0 && (
                      <div className="flex flex-wrap gap-2">
                        {m.audioUrls.map((src, j) => (
                          <audio key={j} src={src} controls className="max-w-full max-h-12" />
                        ))}
                      </div>
                    )}
                    {m.videoUrls && m.videoUrls.length > 0 && (
                      <div className="flex flex-wrap gap-2">
                        {m.videoUrls.map((src, j) => (
                          <video
                            key={j}
                            src={src}
                            controls
                            className="max-w-[280px] max-h-[200px] rounded-lg border border-base-content/10 bg-base-100"
                          />
                        ))}
                      </div>
                    )}
                    {m.text && (
                      <div className="space-y-1">
                        <span className="whitespace-pre-wrap break-words">{m.text}</span>
                        {m.state === 'error' && m.role === 'assistant' && lastPrompt && m.canRetry && (
                          <div className="flex items-center justify-between text-[11px] text-base-content/70 pt-1 border-t border-base-content/10">
                            <span>
                              {m.errorKind === 'network'
                                ? '网络异常，可稍后重试。'
                                : m.errorKind === 'timeout'
                                  ? '请求超时，可缩短问题或稍后重试。'
                                  : m.errorKind === 'server'
                                    ? '后端出现错误，可稍后重试。'
                                    : '发生未知错误。'}
                            </span>
                            <button
                              type="button"
                              className="btn btn-ghost btn-xs rounded-none"
                              disabled={sending}
                              onClick={() => {
                                if (!lastPrompt || sending) return
                                void send(lastPrompt)
                              }}
                            >
                              重试本轮
                            </button>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          )
            )}
            />

            <div className="absolute right-3 bottom-3 flex flex-col gap-2">
              {!atBottom && (
                <button
                  type="button"
                  className="btn btn-primary btn-sm rounded-none shadow-lg"
                  onClick={scrollToBottomNow}
                  title="跳转到底部"
                >
                  跳到底
                </button>
              )}
              <button
                type="button"
                className="btn btn-ghost btn-sm rounded-none bg-base-200/90 border border-base-300 shadow"
                onClick={() => setJumpOpen(true)}
                title="按序号跳转到某条消息"
              >
                跳转
              </button>
            </div>

            {jumpOpen && (
              <div className="absolute inset-0 bg-black/30 flex items-center justify-center p-4">
                <div className="w-full max-w-[420px] bg-base-100 border border-base-300 rounded-none shadow-xl">
                  <div className="px-4 py-3 border-b border-base-300 bg-base-200 flex items-center justify-between">
                    <div className="font-semibold">跳转到消息</div>
                    <button type="button" className="btn btn-ghost btn-sm" onClick={() => setJumpOpen(false)}>
                      关闭
                    </button>
                  </div>
                  <div className="p-4 space-y-3">
                    <div className="text-xs text-base-content/60">
                      输入消息序号（1 - {messages.length}）
                    </div>
                    <input
                      value={jumpValue}
                      onChange={(e) => setJumpValue(e.target.value)}
                      placeholder="例如：1"
                      className="input input-bordered w-full rounded-none"
                      inputMode="numeric"
                    />
                    <div className="flex justify-end gap-2">
                      <button type="button" className="btn btn-ghost rounded-none" onClick={() => setJumpOpen(false)}>
                        取消
                      </button>
                      <button
                        type="button"
                        className="btn btn-primary rounded-none"
                        onClick={() => {
                          const n = Number(jumpValue)
                          if (!Number.isFinite(n)) return
                          const idx = Math.round(n) - 1
                          scrollToIndex(idx)
                          setJumpOpen(false)
                        }}
                      >
                        跳转
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
      <div
        role="separator"
        aria-orientation="horizontal"
        onPointerDown={handleInputResizePointerDown}
        onMouseDown={handleInputResizeMouseDown}
        className="flex-shrink-0 h-3 cursor-row-resize bg-base-300 hover:bg-primary/30 active:bg-primary/40 flex items-center justify-center transition-colors select-none"
        title="拖动调节输入框高度"
      >
        <span className="w-14 h-[3px] rounded-full bg-base-content/40" />
      </div>
      <div
        className="flex-shrink-0 p-4 border-t border-base-300 flex flex-col gap-2"
        style={{ height: inputHeight + 32 + (pendingImages.length || pendingAudios.length || pendingVideos.length ? 52 : 0) }}
      >
        {(pendingImages.length > 0 || pendingAudios.length > 0 || pendingVideos.length > 0) && (
          <div className="flex flex-wrap gap-2 items-center min-h-[48px]">
            <button
              type="button"
              className="btn btn-ghost btn-xs rounded-none"
              onClick={clearAttachments}
              title="清空所有附件"
            >
              清空附件
            </button>
            {pendingImages.map((x, j) => (
              <span key={j} className="relative inline-block">
                <img
                  src={x.previewUrl}
                  alt=""
                  className="max-w-[72px] max-h-[48px] object-contain rounded border border-base-300 bg-base-100"
                />
                <button
                  type="button"
                  aria-label="移除"
                  className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-error text-error-content text-xs flex items-center justify-center hover:bg-error-focus"
                  onClick={() =>
                    setPendingImages((p) => {
                      const item = p[j]
                      if (item) URL.revokeObjectURL(item.previewUrl)
                      return p.filter((_, i) => i !== j)
                    })
                  }
                >
                  ×
                </button>
              </span>
            ))}
            {pendingAudios.map((x, j) => (
              <span key={`a-${j}`} className="relative inline-flex items-center gap-1 rounded border border-base-300 bg-base-100 px-2 py-1">
                <audio src={x.previewUrl} controls className="max-h-10 w-40" />
                <button
                  type="button"
                  aria-label="移除"
                  className="text-error hover:bg-error/20 rounded p-0.5"
                  onClick={() =>
                    setPendingAudios((p) => {
                      const item = p[j]
                      if (item) URL.revokeObjectURL(item.previewUrl)
                      return p.filter((_, i) => i !== j)
                    })
                  }
                >
                  ×
                </button>
              </span>
            ))}
            {pendingVideos.map((x, j) => (
              <span key={`v-${j}`} className="relative inline-block">
                <video src={x.previewUrl} className="max-w-[100px] max-h-[48px] object-contain rounded border border-base-300 bg-base-100" />
                <button
                  type="button"
                  aria-label="移除"
                  className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-error text-error-content text-xs flex items-center justify-center hover:bg-error-focus"
                  onClick={() =>
                    setPendingVideos((p) => {
                      const item = p[j]
                      if (item) URL.revokeObjectURL(item.previewUrl)
                      return p.filter((_, i) => i !== j)
                    })
                  }
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        {attachHint && (
          <div className="text-xs text-base-content/60">
            {attachHint}
            {uploading && (
              <div className="mt-2 flex items-center gap-2">
                <progress className="progress progress-primary w-48" value={uploadPercent} max={100} />
                <span className="tabular-nums">{uploadPercent}%</span>
                <button
                  type="button"
                  className="btn btn-ghost btn-xs rounded-none"
                  onClick={() => uploadAbortRef.current?.abort()}
                >
                  取消上传
                </button>
              </div>
            )}
          </div>
        )}
        <div className="flex gap-3 items-stretch flex-1 min-h-0">
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          className="hidden"
          multiple
          onChange={(e) => {
            const files = e.target.files
            if (!files?.length) return
            void addFiles(files)
            e.target.value = ''
          }}
        />
        <input
          ref={textFileInputRef}
          type="file"
          accept=".txt,.md,.json,.log,text/plain,text/markdown,application/json"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0]
            if (!file) return
            const r = new FileReader()
            r.readAsText(file)
            r.onload = () => {
              const text = r.result as string
              setInput((prev) => (prev ? prev + '\n' + text : text))
            }
            e.target.value = ''
          }}
        />
        <input
          ref={audioInputRef}
          type="file"
          accept="audio/*"
          className="hidden"
          multiple
          onChange={(e) => {
            const files = e.target.files
            if (!files?.length) return
            void addFiles(files)
            e.target.value = ''
          }}
        />
        <input
          ref={videoInputRef}
          type="file"
          accept="video/*"
          className="hidden"
          multiple
          onChange={(e) => {
            const files = e.target.files
            if (!files?.length) return
            void addFiles(files)
            e.target.value = ''
          }}
        />
        <div
          className="chat-input flex-1 min-w-[120px] flex gap-2 items-end border-2 border-base-300 bg-base-100 rounded-xl px-2 pb-2 pt-2 focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/20 transition-all duration-200"
          style={{ minHeight: inputHeight }}
        >
          <div className="flex gap-1.5 shrink-0 pb-0.5">
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
              className="flex-shrink-0 w-11 h-11 min-h-0 rounded-lg btn btn-ghost hover:bg-base-300/60 flex items-center justify-center p-0"
              title="上传图片"
            >
              <span className="inline-flex shrink-0 w-7 h-7 items-center justify-center"><ImagePlus size={28} /></span>
            </button>
            <button
              type="button"
              onClick={() => textFileInputRef.current?.click()}
              className="flex-shrink-0 w-11 h-11 min-h-0 rounded-lg btn btn-ghost hover:bg-base-300/60 flex items-center justify-center p-0"
              title="上传文本文件"
            >
              <span className="inline-flex shrink-0 w-7 h-7 items-center justify-center"><FileText size={28} /></span>
            </button>
            <button
              type="button"
              onClick={() => audioInputRef.current?.click()}
              className="flex-shrink-0 w-11 h-11 min-h-0 rounded-lg btn btn-ghost hover:bg-base-300/60 flex items-center justify-center p-0"
              title="上传音频"
            >
              <span className="inline-flex shrink-0 w-7 h-7 items-center justify-center"><Mic size={28} /></span>
            </button>
            <button
              type="button"
              onClick={() => videoInputRef.current?.click()}
              className="flex-shrink-0 w-11 h-11 min-h-0 rounded-lg btn btn-ghost hover:bg-base-300/60 flex items-center justify-center p-0"
              title="上传视频"
            >
              <span className="inline-flex shrink-0 w-7 h-7 items-center justify-center"><Video size={28} /></span>
            </button>
          </div>
          <textarea
            dir="ltr"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                send()
              }
            }}
            placeholder="输入消息，Enter 发送 / Shift+Enter 换行…"
            rows={1}
            style={{ height: inputHeight - 16, minHeight: MIN_INPUT_HEIGHT - 16, maxHeight: MAX_INPUT_HEIGHT - 16 }}
            className="flex-1 min-w-0 resize-none border-0 bg-transparent pl-0 pr-2 py-1.5 text-left text-[15px] leading-relaxed placeholder:text-base-content/50 focus:outline-none focus:ring-0"
          />
          <div className="flex flex-col items-end gap-1">
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => {
                  void send()
                }}
                disabled={sending}
                className="send-btn flex-shrink-0 w-12 h-12 min-h-0 rounded-xl bg-primary text-primary-content border-0 shadow-md hover:shadow-lg hover:bg-primary-focus active:scale-[0.96] disabled:opacity-60 disabled:cursor-not-allowed disabled:hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 transition-all duration-200 flex items-center justify-center self-end"
                title="发送"
              >
                {sending ? (
                  <Loader2 size={22} className="animate-spin" />
                ) : (
                  <Send size={22} strokeWidth={2.25} />
                )}
              </button>
              <button
                type="button"
                onClick={cancel}
                disabled={!sending}
                className="send-btn flex-shrink-0 w-12 h-12 min-h-0 rounded-xl bg-error text-error-content border-0 shadow-md hover:shadow-lg hover:bg-error/90 active:scale-[0.96] disabled:opacity-60 disabled:cursor-not-allowed disabled:hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-error focus-visible:ring-offset-2 transition-all duration-200 flex items-center justify-center self-end"
                title="中止当前大模型回答及其工具调用（仅作用于本轮）"
              >
                <Square size={20} strokeWidth={2.25} />
              </button>
            </div>
          </div>
        </div>
        </div>
      </div>
    </div>
  )
}
