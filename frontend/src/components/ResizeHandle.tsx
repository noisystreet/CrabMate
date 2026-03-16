import { useRef, useCallback } from 'react'

const MIN_WIDTH = 200
const MAX_WIDTH = 560

interface ResizeHandleProps {
  onResize: (width: number) => void
  containerRef: React.RefObject<HTMLDivElement | null>
}

export function ResizeHandle({ onResize, containerRef }: ResizeHandleProps) {
  const dragging = useRef(false)

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault()
      if (!containerRef.current) return
      dragging.current = true

      const onMouseMove = (moveEvent: MouseEvent) => {
        if (!containerRef.current || !dragging.current) return
        const rect = containerRef.current.getBoundingClientRect()
        const widthFromRight = rect.right - moveEvent.clientX
        const next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, widthFromRight))
        onResize(next)
      }

      const onMouseUp = () => {
        dragging.current = false
        document.removeEventListener('mousemove', onMouseMove)
        document.removeEventListener('mouseup', onMouseUp)
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
      }

      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'
      document.addEventListener('mousemove', onMouseMove)
      document.addEventListener('mouseup', onMouseUp)
    },
    [onResize, containerRef]
  )

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      onMouseDown={handleMouseDown}
      className="w-1 flex-shrink-0 bg-base-300 cursor-col-resize hover:bg-primary/30 transition-colors"
    />
  )
}
