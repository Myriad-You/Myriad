import type {
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
  WheelEvent,
} from 'react'
import { useCallback, useLayoutEffect, useRef } from 'react'
import { onVisibility } from '../../hooks/animation/core'

interface WidgetGestureOptions {
  visible: boolean
  isAdmin: boolean
  editing: boolean
  rows: number
  page: number
  maxPage: number
  onEdit: () => void
  onPrepareEdit: () => void
  onRows: (rows: number) => void
  onPage: (page: number) => void
}

const PRESS_MOVE_TOLERANCE_PX = 8

/** Gesture work belongs to the visible panel, not to the render that started it. */
export function useWidgetGestures(options: WidgetGestureOptions) {
  const latest = useRef(options)
  const press = useRef<ReturnType<typeof setTimeout> | null>(null)
  const wheel = useRef<ReturnType<typeof setTimeout> | null>(null)
  const stopDrag = useRef<(() => void) | null>(null)
  const isDraggingRef = useRef(false)
  /** 长按起点：按住后移动超过这个距离就是拖动（横拖翻页、嵌套小组件的滑动），不是长按。 */
  const pressFrom = useRef<{ x: number; y: number } | null>(null)
  const handleMouseUp = useCallback(() => {
    if (press.current !== null) clearTimeout(press.current)
    press.current = null
  }, [])
  const cancel = useCallback(() => {
    handleMouseUp()
    if (wheel.current !== null) clearTimeout(wheel.current)
    wheel.current = null
    stopDrag.current?.()
  }, [handleMouseUp])

  useLayoutEffect(() => {
    latest.current = options
    if (!options.visible) { cancel()
}
    else if (!options.isAdmin) {
      handleMouseUp()
      stopDrag.current?.()
    } else if (!options.editing) { stopDrag.current?.()
}
  })
  useLayoutEffect(() => {
    const unsubscribe = onVisibility(visible => { if (!visible) cancel() })
    window.addEventListener('blur', cancel)
    return () => {
      unsubscribe()
      window.removeEventListener('blur', cancel)
      cancel()
    }
  }, [cancel])

  const handleMouseDown = useCallback(() => {
    handleMouseUp()
    const current = latest.current
    if (!current.visible || !current.isAdmin || current.editing) return
    current.onPrepareEdit()
    press.current = setTimeout(() => {
      press.current = null
      const now = latest.current
      if (now.visible && now.isAdmin && !now.editing) now.onEdit()
    }, 800)
  }, [handleMouseUp])

  const handleResizeStart = useCallback((event: ReactMouseEvent) => {
    event.preventDefault()
    event.stopPropagation()
    const current = latest.current
    if (!current.visible || !current.isAdmin || !current.editing) return
    handleMouseUp()
    stopDrag.current?.()
    const startY = event.clientY
    const startRows = current.rows
    let lastRows = startRows
    isDraggingRef.current = false
    const move = (next: MouseEvent) => {
      const delta = next.clientY - startY
      if (Math.abs(delta) > 5) isDraggingRef.current = true
      const rows = startRows === 2 ? (delta < -10 ? 1 : 2) : (delta > 10 ? 2 : 1)
      if (rows !== lastRows) {
        lastRows = rows
        latest.current.onRows(rows)
      }
    }
    const stop = () => {
      document.removeEventListener('mousemove', move)
      document.removeEventListener('mouseup', stop)
      stopDrag.current = null
    }
    stopDrag.current = stop
    document.addEventListener('mousemove', move)
    document.addEventListener('mouseup', stop)
  }, [handleMouseUp])

  const handleWheel = useCallback((event: WheelEvent) => {
    const current = latest.current
    // Shift+滚轮是横向手势，归轮换 hook 处理，这里再翻会一次翻两页。
    if (!current.visible || event.shiftKey || wheel.current !== null || Math.abs(event.deltaY) <= 30) return
    const page = Math.max(0, Math.min(current.maxPage, current.page + (event.deltaY > 0 ? 1 : -1)))
    if (page === current.page) return
    current.onPage(page)
    wheel.current = setTimeout(() => { wheel.current = null }, 400)
  }, [])
  // 按下时记起点；指针挪开超过阈值就不再是长按，管理员慢慢横拖也不会顺带进编辑态。
  const handlePointerDown = useCallback((event: ReactPointerEvent) => {
    pressFrom.current = { x: event.clientX, y: event.clientY }
  }, [])
  const handlePointerMove = useCallback((event: ReactPointerEvent) => {
    const from = pressFrom.current
    if (press.current === null || !from) return
    if (Math.hypot(event.clientX - from.x, event.clientY - from.y) > PRESS_MOVE_TOLERANCE_PX) {
      handleMouseUp()
    }
  }, [handleMouseUp])
  return {
    handleMouseDown,
    handleMouseUp,
    handlePointerDown,
    handlePointerMove,
    handleResizeStart,
    handleWheel,
    isDraggingRef,
  }
}
