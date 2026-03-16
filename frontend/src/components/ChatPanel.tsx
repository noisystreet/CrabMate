import { useState, useRef, useEffect, useCallback } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkMath from 'remark-math'
import remarkBreaks from 'remark-breaks'
import remarkGfm from 'remark-gfm'
import rehypeKatex from 'rehype-katex'
import 'katex/dist/katex.min.css'
import { Send, User, Bot, Loader2, ImagePlus, FileText, Mic, Video } from 'lucide-react'

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
  // 先做 LaTeX 预处理
  let s = preprocessLatexBlocks(raw.replace(/\\n/g, '\n'))
  // 若本身已有换行，则做规范化：去掉“伪空行”，压缩连续空行为最多 2 行，避免段落被贴在一起
  if (s.includes('\n')) {
    const norm = s.replace(/\r\n/g, '\n')
    const lines = norm.split('\n')
    const out: string[] = []
    let emptyCount = 0
    for (const line of lines) {
      const isEmpty = line.trim().length === 0
      if (isEmpty) {
        if (emptyCount < 2) {
          out.push('')
        }
        emptyCount += 1
      } else {
        out.push(line)
        emptyCount = 0
      }
    }
    return out.join('\n')
  }
  // 在中文句号、问号、叹号后面尝试插入换行
  s = s.replace(/(。|！|？)/g, '$1\n')
  // 在编号列表前插入换行：1. 2. 3.，并确保有空行 + 空格（更符合 markdown 列表语法）
  s = s.replace(/(\d+\.)(?=\S)/g, '\n\n$1 ')
  // 保持为单个换行，remark-breaks 会将其渲染为 <br>，不会产生多余空行
  return s
}

/** 规范命令输出中的换行，避免前端展示时出现过多空行 */
function normalizeToolOutput(raw: string): string {
  return raw
    .replace(/\r\n/g, '\n')     // 统一换行符
    .replace(/\n{3,}/g, '\n\n') // 连续 3 行以上空行压缩为 1 个空行
    .replace(/\n+$/g, '')       // 去掉末尾多余空行
}
import { sendChatStream } from '../api'

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

type Message = {
  role: 'user' | 'assistant' | 'system'
  text: string
  images?: string[]
  audioUrls?: string[]
  videoUrls?: string[]
  state?: 'loading' | 'error'
  collapsed?: boolean
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
  /** 通知父组件工具运行状态（例如 run_command / run_executable 执行中） */
  onToolStatusChange?: (running: boolean) => void
}

export function ChatPanel({
  onSendStart,
  onSendEnd,
  onWorkspaceChanged,
  exportTrigger = 0,
  onMessagesChange,
  onToolStatusChange,
}: ChatPanelProps) {
  const [input, setInput] = useState('')
  const [messages, setMessages] = useState<Message[]>([])
  const [pendingImages, setPendingImages] = useState<string[]>([])
  const [pendingAudios, setPendingAudios] = useState<string[]>([])
  const [pendingVideos, setPendingVideos] = useState<string[]>([])
  const [sending, setSending] = useState(false)
  const [inputHeight, setInputHeight] = useState(getStoredInputHeight)
  const listRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const textFileInputRef = useRef<HTMLInputElement>(null)
  const audioInputRef = useRef<HTMLInputElement>(null)
  const videoInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    listRef.current?.scrollTo(0, listRef.current.scrollHeight)
  }, [messages])

  useEffect(() => {
    localStorage.setItem(INPUT_HEIGHT_KEY, String(inputHeight))
  }, [inputHeight])

  useEffect(() => {
    onMessagesChange?.(messages.length > 0)
  }, [messages, onMessagesChange])

  useEffect(() => {
    if (exportTrigger > 0) {
      exportConversation()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [exportTrigger])

  const exportConversation = () => {
    if (messages.length === 0) return
    const data = {
      exportedAt: new Date().toISOString(),
      messages: messages.map(({ role, text, state, images, audioUrls, videoUrls }) => ({
        role,
        text,
        state,
        images,
        audioUrls,
        videoUrls,
      })),
    }
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    const date = new Date()
    const ts = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(
      date.getDate(),
    ).padStart(2, '0')}_${String(date.getHours()).padStart(2, '0')}${String(
      date.getMinutes(),
    ).padStart(2, '0')}`
    a.href = url
    a.download = `chat_session_${ts}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
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

  const send = async () => {
    const msg = input.trim()
    const images = pendingImages.length > 0 ? [...pendingImages] : undefined
    const audioUrls = pendingAudios.length > 0 ? [...pendingAudios] : undefined
    const videoUrls = pendingVideos.length > 0 ? [...pendingVideos] : undefined
    if ((!msg && !images && !audioUrls && !videoUrls) || sending) return
    setInput('')
    setPendingImages([])
    setPendingAudios([])
    setPendingVideos([])
    const fallback = images ? '(图片)' : audioUrls ? '(音频)' : videoUrls ? '(视频)' : ''
    setMessages((m) => [...m, { role: 'user', text: msg || fallback, images, audioUrls, videoUrls }])
    setMessages((m) => [...m, { role: 'assistant', text: '', state: 'loading' }])
    setSending(true)
    onSendStart?.()
    let finished = false
    try {
      await sendChatStream(msg, {
        onDelta: (text) => {
          setMessages((m) => {
            const next = [...m]
            // 从末尾向前找到最后一个正在加载的 assistant 消息
            let idx = -1
            for (let j = next.length - 1; j >= 0; j -= 1) {
              if (next[j].role === 'assistant' && next[j].state === 'loading') {
                idx = j
                break
              }
            }
            if (idx >= 0) {
              next[idx] = { ...next[idx], text: (next[idx].text || '') + text }
            }
            return next
          })
        },
        onWorkspaceChanged,
        onToolCall: ({ summary }) => {
          setMessages((m) => [
            ...m,
            {
              role: 'system',
              text: summary,
              collapsed: true,
            },
          ])
        },
        onToolStatusChange,
        onToolResult: ({ name, output }) => {
          // 将工具输出也插入到对话中，使用 system 样式，便于查看 ls 等命令结果
          const header = name ? `命令输出（${name}）：` : '命令输出：'
          setMessages((m) => [
            ...m,
            {
              role: 'system',
              text: `${header}\n${normalizeToolOutput(output)}`,
              collapsed: false,
            },
          ])
        },
        onDone: () => {
          finished = true
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
              const t = next[idx].text?.trim() || ''
              next[idx] = { role: 'assistant', text: t || '(无回复)' }
            }
            return next
          })
          onSendEnd?.()
          setSending(false)
        },
        onError: (errMsg) => {
          finished = true
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
              next[idx] = { role: 'assistant', text: '错误：' + errMsg, state: 'error' }
            }
            return next
          })
          onSendEnd?.(errMsg)
          setSending(false)
        },
      })
    } catch (e) {
      const msgText = e instanceof Error ? e.message : '请求失败'
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
          next[idx] = { role: 'assistant', text: '错误：' + msgText, state: 'error' }
        }
        return next
      })
      finished = true
      onSendEnd?.(msgText)
      setSending(false)
    } finally {
      // 兜底：若 sendChatStream 正常返回但未触发 onDone/onError（例如某些异常路径），
      // 也要确保结束本轮忙碌状态，避免状态栏一直显示“生成中”
      if (!finished) {
        onSendEnd?.()
        setSending(false)
      }
    }
  }

  return (
    <div ref={panelRef} className="card flex flex-col h-full min-h-0 bg-base-200 border border-base-300 border-r-0 border-b-0 shadow-none rounded-none">
      <div ref={listRef} className="flex-1 min-h-0 overflow-y-auto p-4 space-y-4">
        {messages.length === 0 && (
          <p className="text-base-content/60 text-sm">输入消息并发送，开始对话</p>
        )}
        {messages.map((m, i) =>
          m.role === 'system' ? (
            <div key={i} className="flex justify-center text-xs text-base-content/60">
              <button
                type="button"
                className="inline-flex items-center gap-1 px-2 py-1 rounded-full bg-base-300/80 hover:bg-base-300 transition-colors"
                onClick={() =>
                  setMessages((prev) => {
                    const next = [...prev]
                    next[i] = { ...next[i], collapsed: !next[i].collapsed }
                    return next
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
              key={i}
              className={`flex gap-3 items-end ${m.role === 'user' ? 'flex-row-reverse' : ''}`}
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
                    <span className="whitespace-pre-wrap break-words">{m.text || '\u00A0'}</span>
                  </div>
                ) : m.role === 'assistant' && !m.state ? (
                  <div className="markdown-body text-[15px] leading-relaxed">
                    <ReactMarkdown
                      remarkPlugins={[remarkMath, remarkBreaks, remarkGfm]}
                      rehypePlugins={[rehypeKatex]}
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
                    {m.text ? <span className="whitespace-pre-wrap break-words">{m.text}</span> : null}
                  </div>
                )}
              </div>
            </div>
          ),
        )}
      </div>
      <div
        role="separator"
        aria-orientation="horizontal"
        onMouseDown={handleInputResizeMouseDown}
        className="flex-shrink-0 h-1.5 cursor-row-resize hover:bg-primary/30 bg-base-300 flex items-center justify-center transition-colors"
        title="拖动调节输入框高度"
      >
        <span className="w-10 h-0.5 rounded-full bg-base-content/30" />
      </div>
      <div
        className="flex-shrink-0 p-4 border-t border-base-300 flex flex-col gap-2"
        style={{ height: inputHeight + 32 + (pendingImages.length || pendingAudios.length || pendingVideos.length ? 52 : 0) }}
      >
        {(pendingImages.length > 0 || pendingAudios.length > 0 || pendingVideos.length > 0) && (
          <div className="flex flex-wrap gap-2 items-center min-h-[48px]">
            {pendingImages.map((src, j) => (
              <span key={j} className="relative inline-block">
                <img
                  src={src}
                  alt=""
                  className="max-w-[72px] max-h-[48px] object-contain rounded border border-base-300 bg-base-100"
                />
                <button
                  type="button"
                  aria-label="移除"
                  className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-error text-error-content text-xs flex items-center justify-center hover:bg-error-focus"
                  onClick={() => setPendingImages((p) => p.filter((_, i) => i !== j))}
                >
                  ×
                </button>
              </span>
            ))}
            {pendingAudios.map((src, j) => (
              <span key={`a-${j}`} className="relative inline-flex items-center gap-1 rounded border border-base-300 bg-base-100 px-2 py-1">
                <audio src={src} controls className="max-h-10 w-40" />
                <button type="button" aria-label="移除" className="text-error hover:bg-error/20 rounded p-0.5" onClick={() => setPendingAudios((p) => p.filter((_, i) => i !== j))}>×</button>
              </span>
            ))}
            {pendingVideos.map((src, j) => (
              <span key={`v-${j}`} className="relative inline-block">
                <video src={src} className="max-w-[100px] max-h-[48px] object-contain rounded border border-base-300 bg-base-100" />
                <button type="button" aria-label="移除" className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-error text-error-content text-xs flex items-center justify-center hover:bg-error-focus" onClick={() => setPendingVideos((p) => p.filter((_, i) => i !== j))}>×</button>
              </span>
            ))}
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
            const readers = Array.from(files).filter((f) => f.type.startsWith('image/')).map((f) => {
              const r = new FileReader()
              r.readAsDataURL(f)
              return new Promise<string>((res, rej) => {
                r.onload = () => res(r.result as string)
                r.onerror = rej
              })
            })
            Promise.all(readers).then((urls) => {
              setPendingImages((prev) => [...prev, ...urls])
            })
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
            const readers = Array.from(files).filter((f) => f.type.startsWith('audio/')).map((f) => {
              const r = new FileReader()
              r.readAsDataURL(f)
              return new Promise<string>((res, rej) => {
                r.onload = () => res(r.result as string)
                r.onerror = rej
              })
            })
            Promise.all(readers).then((urls) => setPendingAudios((prev) => [...prev, ...urls]))
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
            const readers = Array.from(files).filter((f) => f.type.startsWith('video/')).map((f) => {
              const r = new FileReader()
              r.readAsDataURL(f)
              return new Promise<string>((res, rej) => {
                r.onload = () => res(r.result as string)
                r.onerror = rej
              })
            })
            Promise.all(readers).then((urls) => setPendingVideos((prev) => [...prev, ...urls]))
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
          <button
            type="button"
            onClick={send}
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
        </div>
        </div>
      </div>
    </div>
  )
}
