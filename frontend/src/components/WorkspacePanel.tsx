import { useState, useEffect, useCallback } from 'react'
import { FolderOpen, FileText, Loader2, Settings, FilePlus, Save, Download, Trash2 } from 'lucide-react'
import { fetchWorkspace, fetchWorkspacePick, fetchWorkspaceFile, writeWorkspaceFile, setWorkspacePath, deleteWorkspaceFile } from '../api'
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
}

export function WorkspacePanel({ width = 280, refreshTrigger = 0 }: WorkspacePanelProps) {
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

  const loadWorkspace = useCallback(() => {
    if (workspaceDir === null) return
    setLoading(true)
    setError(null)
    fetchWorkspace(workspaceDir === '' ? undefined : workspaceDir)
      .then((d) => {
        setData(d)
        setError(d.error ?? null)
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
      }
    } catch {
      // 认为当前环境无法正常弹出目录选择框（例如无 GUI 的服务器环境），后续禁用按钮并提示手动输入
      setPickSupported(false)
      setError('当前环境无法打开目录选择框，请在输入框中手动输入路径')
    } finally {
      setPickLoading(false)
    }
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
            <button
              type="button"
              className="btn btn-ghost btn-xs btn-square"
              title="新建文件"
              onClick={openCreateFile}
            >
              <FilePlus size={14} />
            </button>
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
      <div className="flex-shrink-0 px-4 py-2 text-xs text-base-content/60 break-all font-mono">
        {workspaceDir === null ? '未设置' : data?.path ? data.path : '—'}
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {workspaceDir === null && (
          <p className="text-base-content/60 text-sm py-4 px-2 leading-relaxed">
            工作区未设置。请点击右上角设置图标，选择要浏览的目录；留空则使用服务端默认目录。
          </p>
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
            {data.entries.length === 0 ? (
              <p className="text-base-content/60 text-sm py-4 px-2">（空）</p>
            ) : (
              <ul className="space-y-0.5">
                {data.entries.map((e) => (
                  <li
                    key={e.name}
                    className={`flex items-center gap-2 px-2 py-1.5 rounded-none text-sm text-base-content transition-colors ${e.is_dir ? '' : 'hover:bg-base-300 cursor-pointer'}`}
                    onClick={e.is_dir ? undefined : () => openEditFile(data.path, e.name)}
                    onKeyDown={e.is_dir ? undefined : (ev) => ev.key === 'Enter' && openEditFile(data.path, e.name)}
                    role={e.is_dir ? undefined : 'button'}
                    tabIndex={e.is_dir ? undefined : 0}
                    onContextMenu={e.is_dir ? undefined : (ev) => handleFileContextMenu(ev, data.path, e.name)}
                  >
                    {e.is_dir ? (
                      <FolderOpen size={16} className="text-warning flex-shrink-0" />
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
    </div>
  )
}
