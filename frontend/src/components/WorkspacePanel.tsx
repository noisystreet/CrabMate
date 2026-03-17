import { useState, useEffect, useCallback } from 'react'
import { FolderOpen, FileText, Loader2, Settings, FilePlus, Save, Download, Trash2, ChevronRight, Home, Search, Copy, Terminal } from 'lucide-react'
import { fetchWorkspace, fetchWorkspacePick, fetchWorkspaceFile, writeWorkspaceFile, setWorkspacePath, deleteWorkspaceFile, searchWorkspace } from '../api'
import type { WorkspaceData } from '../types'

function joinPath(dir: string, name: string): string {
  const d = dir.replace(/\\/g, '/').replace(/\/+$/, '')
  const n = name.replace(/^\/+/, '')
  return n ? `${d}/${n}` : d
}

const WORKSPACE_DIR_KEY = 'agent-demo-workspace-dir'

/** null = 从未设置（首次打开）；'' = 使用默认目录；string = 指定路径 */
function getStoredWorkspaceDir(): string | null {
  if (typeof window === 'undefined') return null
  const v = localStorage.getItem(WORKSPACE_DIR_KEY)
  return v === null ? null : v
}

interface WorkspacePanelProps {
  width?: number
  /** 当此值增加时刷新工作区列表（例如 Agent 创建/修改文件后由父组件递增） */
  refreshTrigger?: number
  /** 将文本发送到聊天面板作为新一轮 user 消息 */
  onSendToChat?: (text: string) => void
}

export function WorkspacePanel({ width = 280, refreshTrigger = 0, onSendToChat }: WorkspacePanelProps) {
  const [workspaceDir, setWorkspaceDir] = useState<string | null>(() => getStoredWorkspaceDir())
  const [data, setData] = useState<WorkspaceData | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [dirInput, setDirInput] = useState('')
  const [pickLoading, setPickLoading] = useState(false)
  const [pickSupported, setPickSupported] = useState(true)
  const [fileModal, setFileModal] = useState<'create' | 'edit' | null>(null)
  const [filePath, setFilePath] = useState('')
  const [fileContent, setFileContent] = useState('')
  const [fileSaving, setFileSaving] = useState(false)
  const [fileError, setFileError] = useState<string | null>(null)
  const [contextMenu, setContextMenu] = useState<{
    path: string
    name: string
    x: number
    y: number
  } | null>(null)

  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(() => new Set())
  const [currentDir, setCurrentDir] = useState<string | null>(null)

  const [searchModalOpen, setSearchModalOpen] = useState(false)
  const [searchPattern, setSearchPattern] = useState('')
  const [searchResult, setSearchResult] = useState('')
  const [searchLoading, setSearchLoading] = useState(false)
  const [searchError, setSearchError] = useState<string | null>(null)

  const [activeFile, setActiveFile] = useState<string | null>(null)
  const [recentFiles, setRecentFiles] = useState<string[]>([])

  const loadWorkspace = useCallback(() => {
    if (workspaceDir === null) return
    setLoading(true)
    setError(null)
    fetchWorkspace(workspaceDir === '' ? undefined : workspaceDir)
      .then((d) => {
        setData(d)
        setError(d.error ?? null)
        setCurrentDir(d.path || null)
      })
      .catch((e) => setError(e instanceof Error ? e.message : '加载失败'))
      .finally(() => setLoading(false))
  }, [workspaceDir])

  useEffect(() => {
    loadWorkspace()
  }, [loadWorkspace])

  useEffect(() => {
    if (refreshTrigger > 0) loadWorkspace()
  }, [refreshTrigger, loadWorkspace])

  // 初次加载时不再自动把历史工作区同步到后端，
  // 只在用户明确设置 / 选择目录时才调用 setWorkspacePath。

  const applyDir = async () => {
    const v = dirInput.trim()
    // 为空表示「使用默认目录」，非空为指定路径；同时将该选择持久化
    setWorkspaceDir(v === '' ? '' : v)
    localStorage.setItem(WORKSPACE_DIR_KEY, v)
    setMenuOpen(false)
    setDirInput('')
    try {
      await setWorkspacePath(v)
    } catch {
      setError('同步工作区到服务端失败')
    }
  }

  const handlePickFolder = async () => {
    if (!pickSupported) {
      setError('当前环境不支持目录选择，请在输入框中手动输入路径')
      return
    }
    setPickLoading(true)
    try {
      const { path } = await fetchWorkspacePick()
      if (path) {
        setWorkspaceDir(path)
        localStorage.setItem(WORKSPACE_DIR_KEY, path)
        setMenuOpen(false)
        setDirInput('')
        await setWorkspacePath(path)
        setExpandedDirs(new Set())
        setCurrentDir(path)
      }
    } catch {
      // 认为当前环境无法正常弹出目录选择框（例如无 GUI 的服务器环境），后续禁用按钮并提示手动输入
      setPickSupported(false)
      setError('当前环境无法打开目录选择框，请在输入框中手动输入路径')
    } finally {
      setPickLoading(false)
    }
  }

  const toggleDir = (path: string) => {
    setExpandedDirs((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const changeDir = (path: string | null) => {
    if (!data) return
    const base = data.path
    const target = path ?? base
    if (!target || !base) return
    // 只改变视图层级，不重新请求服务器；entries 始终是 base 下第一层
    setCurrentDir(target)
  }

  const buildBreadcrumbs = () => {
    if (!data?.path || !currentDir) return []
    const base = data.path
    const rel = currentDir.startsWith(base) ? currentDir.slice(base.length).replace(/^\/+/, '') : ''
    if (!rel) return []
    const parts = rel.split('/').filter(Boolean)
    const items: { label: string; path: string | null }[] = []
    let acc = base
    parts.forEach((p) => {
      acc = `${acc}/${p}`
      items.push({ label: p, path: acc })
    })
    return items
  }

  const visibleEntries = () => {
    if (!data?.entries || !data.path || !currentDir || currentDir === data.path) return data?.entries || []
    const rel = currentDir.slice(data.path.length).replace(/^\/+/, '')
    if (!rel) return data.entries
    const segs = rel.split('/').filter(Boolean)
    // 简化：只支持一层嵌套视图，匹配当前目录名
    const currentName = segs[segs.length - 1]
    const dirEntry = data.entries.find((e) => e.is_dir && e.name === currentName)
    if (!dirEntry || !dirEntry.children) return data.entries
    return dirEntry.children
  }

  const openCreateFile = () => {
    if (!data?.path) return
    setFilePath('')
    setFileContent('')
    setFileError(null)
    setFileModal('create')
  }

  const openEditFile = async (dirPath: string, name: string) => {
    const path = joinPath(dirPath, name)
    setFilePath(path)
    setFileError(null)
    setFileModal('edit')
    setFileContent('')
    try {
      const res = await fetchWorkspaceFile(path)
      if (res.error) setFileError(res.error)
      else setFileContent(res.content)
    } catch (e) {
      setFileError(e instanceof Error ? e.message : '加载失败')
    }
    setActiveFile(path)
    setRecentFiles((prev) => {
      const next = [path, ...prev.filter((p) => p !== path)]
      return next.slice(0, 5)
    })
  }

  const openEditFileByFullPath = (fullPath: string) => {
    const norm = fullPath.replace(/\\/g, '/')
    const idx = norm.lastIndexOf('/')
    if (idx <= 0) return
    const dir = norm.slice(0, idx)
    const name = norm.slice(idx + 1)
    if (!dir || !name) return
    void openEditFile(dir, name)
  }

  const saveFile = async () => {
    const pathToSave = fileModal === 'create' ? (data?.path ? joinPath(data.path, filePath.trim()) : '') : filePath
    if (!pathToSave.trim()) {
      setFileError('文件名不能为空')
      return
    }
    setFileSaving(true)
    setFileError(null)
    try {
      const res = await writeWorkspaceFile(pathToSave, fileContent)
      if (res.error) setFileError(res.error)
      else {
        setFileModal(null)
        loadWorkspace()
      }
    } catch (e) {
      setFileError(e instanceof Error ? e.message : '保存失败')
    } finally {
      setFileSaving(false)
    }
  }

  const closeFileModal = () => {
    setFileModal(null)
    setFileError(null)
  }

  const handleFileContextMenu = (e: React.MouseEvent, dirPath: string, name: string) => {
    e.preventDefault()
    if (!data?.path) return
    const path = joinPath(dirPath, name)
    setContextMenu({
      path,
      name,
      x: e.clientX,
      y: e.clientY,
    })
  }

  const handleDownloadFile = async () => {
    if (!contextMenu) return
    try {
      const res = await fetchWorkspaceFile(contextMenu.path)
      if (res.error) {
        setError(res.error)
        return
      }
      const blob = new Blob([res.content], { type: 'text/plain;charset=utf-8' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = contextMenu.name || 'file.txt'
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
    } catch (e) {
      setError(e instanceof Error ? e.message : '下载失败')
    } finally {
      setContextMenu(null)
    }
  }

  const handleDeleteFile = async () => {
    if (!contextMenu) return
    try {
      const res = await deleteWorkspaceFile(contextMenu.path)
      if (res.error) {
        setError(res.error)
      } else {
        loadWorkspace()
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : '删除失败')
    } finally {
      setContextMenu(null)
    }
  }

  return (
    <div
      className="card flex-shrink-0 flex flex-col flex-1 min-h-0 bg-base-200 border border-base-300 border-b-0 shadow-none rounded-none"
      style={{ width: `${width}px` }}
    >
      <header className="flex-shrink-0 px-4 py-3 border-b border-base-300 flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-base-content">工作区</h2>
        <div className="flex items-center gap-1">
          {workspaceDir !== null && data && !data.error && (
            <>
              <button
                type="button"
                className="btn btn-ghost btn-xs btn-square"
                title="在当前目录搜索"
                onClick={() => {
                  setSearchPattern('')
                  setSearchResult('')
                  setSearchError(null)
                  setSearchModalOpen(true)
                }}
              >
                <Search size={14} />
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-xs btn-square"
                title="新建文件"
                onClick={openCreateFile}
              >
                <FilePlus size={14} />
              </button>
            </>
          )}
          <div className="dropdown dropdown-end">
          <label
            tabIndex={0}
            className="btn btn-ghost btn-xs btn-square"
            title="设置工作区目录"
            onClick={() => {
              setMenuOpen(true)
              setDirInput(workspaceDir ?? '')
            }}
          >
            <Settings size={14} />
          </label>
          {menuOpen && (
            <>
              <div
                tabIndex={0}
                className="dropdown-content menu p-3 bg-base-200 border border-base-300 w-72 rounded-none shadow-lg z-50"
              >
                <p className="text-xs text-base-content/70 mb-2">设置工作区目录（可浏览选择或输入路径，留空使用默认）</p>
                <button
                  type="button"
                  className="btn btn-outline btn-sm w-full rounded-none mb-2"
                  onClick={handlePickFolder}
                  disabled={pickLoading || !pickSupported}
                >
                  {pickSupported
                    ? pickLoading
                      ? '正在打开选择框…'
                      : '浏览选择目录'
                    : '当前不可用，请手动输入路径'}
                </button>
                <input
                  type="text"
                  value={dirInput}
                  onChange={(e) => setDirInput(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && applyDir()}
                  placeholder="或输入路径，留空为默认"
                  className="input input-bordered input-sm w-full rounded-none mb-2"
                />
                <div className="flex gap-2">
                  <button type="button" className="btn btn-primary btn-sm flex-1 rounded-none" onClick={applyDir}>
                    确定
                  </button>
                  <button
                    type="button"
                    className="btn btn-outline btn-sm rounded-none"
                    onClick={() => {
                      // 清除本地记忆的工作区路径，并让前端回到「未设置」状态
                      setWorkspaceDir(null)
                      localStorage.removeItem(WORKSPACE_DIR_KEY)
                      setMenuOpen(false)
                      setDirInput('')
                      setData(null)
                      setError(null)
                    }}
                  >
                    清除设置
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm rounded-none"
                    onClick={() => { setMenuOpen(false); setDirInput(''); }}
                  >
                    取消
                  </button>
                </div>
              </div>
              <div
                className="fixed inset-0 z-40"
                aria-hidden
                onClick={() => { setMenuOpen(false); setDirInput(''); }}
              />
            </>
          )}
          </div>
        </div>
      </header>
      <div className="flex-shrink-0 px-4 py-2 text-xs text-base-content/60 break-all font-mono flex items-center justify-between gap-2">
        <span className="truncate">
          {workspaceDir === null ? '未设置' : data?.path ? data.path : '—'}
        </span>
        {workspaceDir !== null && data?.path && (
          <span className="flex items-center gap-1 flex-shrink-0">
            <button
              type="button"
              className="btn btn-ghost btn-xs btn-square"
              title="复制当前路径"
              onClick={() => navigator.clipboard.writeText(data.path)}
            >
              <Copy size={12} />
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-xs btn-square"
              title="复制在终端中打开该目录的命令（cd ...）"
              onClick={() => navigator.clipboard.writeText(`cd "${data.path}"`)}
            >
              <Terminal size={12} />
            </button>
          </span>
        )}
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {workspaceDir === null && (
          <div className="flex flex-col items-center justify-center text-base-content/60 text-sm py-6 px-2 gap-2">
            <FolderOpen size={20} className="opacity-40" />
            <p className="text-center leading-relaxed">
              工作区未设置。请点击右上角设置图标，选择要浏览的目录；留空则使用服务端默认目录。
            </p>
          </div>
        )}
        {workspaceDir !== null && loading && (
          <div className="flex items-center gap-2 text-base-content/60 py-4 px-2">
            <Loader2 size={16} className="animate-spin" />
            <span className="text-sm">加载中…</span>
          </div>
        )}
        {workspaceDir !== null && !loading && error && (
          <p className="text-sm text-error py-4 px-2">加载失败：{error}</p>
        )}
        {workspaceDir !== null && !loading && !error && data && !data.error && (
          <>
            {/* 面包屑 */}
            <div className="px-2 pb-2 text-xs text-base-content/70 flex items-center gap-1 flex-wrap">
              <button
                type="button"
                className="inline-flex items-center gap-1 hover:underline"
                onClick={() => {
                  loadWorkspace()
                }}
              >
                <Home size={12} />
                <span>根目录</span>
              </button>
              {(() => {
                const full = data.path || ''
                const parts = full.split('/').filter(Boolean)
                const items: { label: string; path: string }[] = []
                let acc = ''
                parts.forEach((p) => {
                  acc = acc ? `${acc}/${p}` : p
                  items.push({ label: p, path: acc })
                })
                return items.map((b, idx) => (
                  <span key={b.path} className="inline-flex items-center gap-1">
                    <ChevronRight size={10} />
                    {idx === items.length - 1 ? (
                      <span className="truncate max-w-[120px]">{b.label}</span>
                    ) : (
                      <button
                        type="button"
                        className="hover:underline truncate max-w-[120px]"
                        onClick={async () => {
                          try {
                            const d = await fetchWorkspace(b.path)
                            setData(d)
                            setError(d.error ?? null)
                          } catch (e) {
                            setError(e instanceof Error ? e.message : '加载失败')
                          }
                        }}
                      >
                        {b.label}
                      </button>
                    )}
                  </span>
                ))
              })()}
            </div>
            {/* 最近打开文件 */}
            {recentFiles.length > 0 && (
              <div className="px-2 pb-1 text-[11px] text-base-content/60 flex items-center gap-1 flex-wrap">
                <span>最近打开：</span>
                {recentFiles.map((p) => (
                  <button
                    key={p}
                    type="button"
                    className="px-1.5 py-0.5 border border-base-300 rounded-none hover:bg-base-300/70 text-[11px] font-mono max-w-[200px] truncate"
                    title={p}
                    onClick={() => openEditFileByFullPath(p)}
                  >
                    {p}
                  </button>
                ))}
              </div>
            )}
            {/* 当前目录内容列表 */}
            {data.entries.length === 0 ? (
              <div className="flex flex-col items-center justify-center text-base-content/60 text-sm py-6 px-2 gap-2">
                <FolderOpen size={20} className="opacity-40" />
                <p className="text-center">当前目录为空。</p>
              </div>
            ) : (
              <ul className="space-y-0.5">
                {data.entries.map((e) => (
                  <li
                    key={e.name}
                    className={`flex items-center gap-2 px-2 py-1.5 rounded-none text-sm text-base-content transition-colors cursor-pointer ${
                      e.is_dir
                        ? 'hover:bg-base-300'
                        : activeFile && data.path && joinPath(data.path, e.name) === activeFile
                          ? 'bg-base-300/70'
                          : 'hover:bg-base-300'
                    }`}
                    onClick={async () => {
                      if (e.is_dir) {
                        try {
                          const nextPath = joinPath(data.path, e.name)
                          const d = await fetchWorkspace(nextPath)
                          setData(d)
                          setError(d.error ?? null)
                        } catch (err) {
                          setError(err instanceof Error ? err.message : '加载失败')
                        }
                      } else {
                        openEditFile(data.path, e.name)
                      }
                    }}
                    onKeyDown={(ev) => {
                      if (ev.key === 'Enter') {
                        if (e.is_dir) {
                          ;(async () => {
                            try {
                              const nextPath = joinPath(data.path, e.name)
                              const d = await fetchWorkspace(nextPath)
                              setData(d)
                              setError(d.error ?? null)
                            } catch (err) {
                              setError(err instanceof Error ? err.message : '加载失败')
                            }
                          })()
                        } else {
                          openEditFile(data.path, e.name)
                        }
                      }
                    }}
                    role="button"
                    tabIndex={0}
                    onContextMenu={e.is_dir ? undefined : (ev) => handleFileContextMenu(ev, data.path, e.name)}
                  >
                    {e.is_dir ? (
                      <>
                        <span className="flex-shrink-0">
                          <ChevronRight size={12} className="opacity-60" />
                        </span>
                        <FolderOpen size={16} className="text-warning flex-shrink-0" />
                      </>
                    ) : (
                      <FileText size={16} className="text-base-content/50 flex-shrink-0" />
                    )}
                    <span className="truncate">{e.name}</span>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>

      {contextMenu && (
        <>
          <div
            className="fixed inset-0 z-40"
            aria-hidden
            onClick={() => setContextMenu(null)}
          />
          <div
            className="fixed z-50 bg-base-200 border border-base-300 rounded-none shadow-xl text-sm"
            style={{ top: contextMenu.y, left: contextMenu.x }}
          >
            <ul className="menu menu-sm p-1">
              <li>
                <button type="button" onClick={handleDownloadFile}>
                  <Download size={14} />
                  <span>下载文件</span>
                </button>
              </li>
              <li>
                <button type="button" className="text-error" onClick={handleDeleteFile}>
                  <Trash2 size={14} />
                  <span>删除文件</span>
                </button>
              </li>
            </ul>
          </div>
        </>
      )}

      {fileModal && (
        <>
          <div className="fixed inset-0 bg-black/50 z-50" aria-hidden onClick={closeFileModal} />
          <div className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[90vw] max-w-lg max-h-[85vh] flex flex-col bg-base-200 border border-base-300 rounded-lg shadow-xl">
            <div className="flex-shrink-0 px-4 py-3 border-b border-base-300 flex items-center justify-between">
              <span className="font-semibold text-base-content">{fileModal === 'create' ? '新建文件' : '编辑文件'}</span>
              <button type="button" className="btn btn-ghost btn-sm btn-square" onClick={closeFileModal} aria-label="关闭">×</button>
            </div>
            <div className="flex-1 min-h-0 flex flex-col p-4 gap-3 overflow-hidden">
              {fileModal === 'create' && (
                <div>
                  <label className="text-xs text-base-content/70 block mb-1">文件名（当前目录下）</label>
                  <input
                    type="text"
                    value={filePath}
                    onChange={(e) => setFilePath(e.target.value)}
                    placeholder="例如：newfile.txt"
                    className="input input-bordered input-sm w-full rounded-none"
                  />
                </div>
              )}
              <div className="flex-1 min-h-0 flex flex-col">
                <label className="text-xs text-base-content/70 block mb-1">内容</label>
                <textarea
                  value={fileContent}
                  onChange={(e) => setFileContent(e.target.value)}
                  className="textarea textarea-bordered flex-1 min-h-[200px] w-full rounded-none font-mono text-sm"
                  placeholder="文件内容…"
                  spellCheck={false}
                />
              </div>
              {fileError && <p className="text-sm text-error">{fileError}</p>}
              <div className="flex gap-2 justify-end">
                <button type="button" className="btn btn-ghost btn-sm rounded-none" onClick={closeFileModal}>取消</button>
                <button type="button" className="btn btn-primary btn-sm rounded-none gap-1" onClick={saveFile} disabled={fileSaving}>
                  {fileSaving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
                  保存
                </button>
              </div>
            </div>
          </div>
        </>
      )}

      {searchModalOpen && (
        <>
          <div
            className="fixed inset-0 bg-black/40 z-50"
            aria-hidden
            onClick={() => {
              setSearchModalOpen(false)
            }}
          />
          <div className="fixed left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2 w-[90vw] max-w-3xl max-h-[85vh] flex flex-col bg-base-200 border border-base-300 rounded-lg shadow-xl">
            <div className="flex-shrink-0 px-4 py-3 border-b border-base-300 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <Search size={16} />
                <span className="font-semibold text-base-content text-sm">在当前目录搜索</span>
              </div>
              <button
                type="button"
                className="btn btn-ghost btn-sm btn-square"
                aria-label="关闭"
                onClick={() => setSearchModalOpen(false)}
              >
                ×
              </button>
            </div>
            <div className="flex-1 min-h-0 flex flex-col p-4 gap-3 overflow-hidden">
              <div className="flex flex-col gap-2">
                <label className="text-xs text-base-content/70">
                  搜索模式（支持正则）：会在
                  <span className="font-mono mx-1">
                    {data?.path || '(未知目录)'}
                  </span>
                  下递归搜索
                </label>
                <input
                  type="text"
                  className="input input-bordered input-sm w-full rounded-none"
                  placeholder="例如：main\\(|TODO|fn\\s+run_agent_turn"
                  value={searchPattern}
                  onChange={(e) => setSearchPattern(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault()
                      ;(async () => {
                        if (!data?.path || !searchPattern.trim()) return
                        setSearchLoading(true)
                        setSearchError(null)
                        try {
                          const res = await searchWorkspace({
                            pattern: searchPattern.trim(),
                            path: data.path,
                          })
                          setSearchResult(res.output || '(无结果)')
                        } catch (err) {
                          setSearchError(err instanceof Error ? err.message : '搜索失败')
                          setSearchResult('')
                        } finally {
                          setSearchLoading(false)
                        }
                      })()
                    }
                  }}
                />
                <div className="flex gap-2 justify-end">
                  <button
                    type="button"
                    className="btn btn-outline btn-sm rounded-none"
                    onClick={async () => {
                      if (!data?.path || !searchPattern.trim()) return
                      setSearchLoading(true)
                      setSearchError(null)
                      try {
                        const res = await searchWorkspace({
                          pattern: searchPattern.trim(),
                          path: data.path,
                        })
                        setSearchResult(res.output || '(无结果)')
                      } catch (err) {
                        setSearchError(err instanceof Error ? err.message : '搜索失败')
                        setSearchResult('')
                      } finally {
                        setSearchLoading(false)
                      }
                    }}
                    disabled={searchLoading || !searchPattern.trim()}
                  >
                    {searchLoading ? <Loader2 size={14} className="animate-spin" /> : '搜索'}
                  </button>
                </div>
              </div>
              {searchError && <p className="text-xs text-error">{searchError}</p>}
              <div className="flex-1 min-h-0 mt-2">
                <label className="text-xs text-base-content/70 block mb-1">搜索结果</label>
                <pre className="w-full h-full max-h-[340px] overflow-auto bg-base-100 border border-base-300 rounded-none p-2 text-[11px] font-mono whitespace-pre-wrap leading-relaxed">
                  {searchResult || '（尚未搜索）'}
                </pre>
                <div className="mt-2 flex justify-end">
                  <button
                    type="button"
                    className="btn btn-ghost btn-xs rounded-none"
                    disabled={!searchResult.trim()}
                    onClick={() => {
                      if (!searchResult.trim()) return
                      onSendToChat?.(
                        `下面是我在工作区当前目录（${data?.path ?? ''}）中执行 grep 搜索得到的结果，请帮我分析并给出下一步建议：\n\n` +
                          searchResult,
                      )
                      setSearchModalOpen(false)
                    }}
                  >
                    将结果发送到聊天
                  </button>
                </div>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  )
}
