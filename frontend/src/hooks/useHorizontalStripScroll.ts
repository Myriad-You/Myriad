import type {
  CSSProperties,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
  WheelEvent as ReactWheelEvent,
  RefObject,
} from 'react'
import { useCallback, useEffect, useRef, useState } from 'react'

const DRAG_THRESHOLD_PX = 6

export interface HorizontalStripScrollBind {
  ref: RefObject<HTMLDivElement | null>
  onWheel: (e: ReactWheelEvent<HTMLDivElement>) => void
  onPointerDown: (e: ReactPointerEvent<HTMLDivElement>) => void
  onPointerMove: (e: ReactPointerEvent<HTMLDivElement>) => void
  onPointerUp: (e: ReactPointerEvent<HTMLDivElement>) => void
  onPointerCancel: (e: ReactPointerEvent<HTMLDivElement>) => void
  onClickCapture: (e: ReactMouseEvent<HTMLDivElement>) => void
  className: string
  style: CSSProperties | undefined
  isDragging: boolean
}

/** 原生 overflow-x 不管普通鼠标滚轮；桌面需要把纵向滚轮映射成横向，并支持拖拽。 */
export function useHorizontalStripScroll(): HorizontalStripScrollBind {
  const ref = useRef<HTMLDivElement | null>(null)
  const [isDragging, setIsDragging] = useState(false)

  const dragRef = useRef({
    pointerId: -1,
    startX: 0,
    startScrollLeft: 0,
    moved: false,
    active: false,

    captured: false,
  })

  const suppressClickRef = useRef(false)

  const winListenersRef = useRef<(() => void) | null>(null)

  const removeWinListeners = useCallback(() => {
    winListenersRef.current?.()
    winListenersRef.current = null
  }, [])

  const endDrag = useCallback(
    (el: HTMLDivElement | null, pointerId: number) => {
      const state = dragRef.current
      if (!state.active) return

      if (state.moved) {
        suppressClickRef.current = true
      }

      removeWinListeners()

      if (
        el &&
        state.captured &&
        state.pointerId === pointerId &&
        el.hasPointerCapture?.(pointerId)
      ) {
        try {
          el.releasePointerCapture(pointerId)
        } catch {
          /* already released */
        }
      }

      state.active = false
      state.moved = false
      state.captured = false
      state.pointerId = -1
      setIsDragging(false)
    },
    [removeWinListeners],
  )

  useEffect(() => () => removeWinListeners(), [removeWinListeners])

  const applyDragScroll = useCallback((clientX: number) => {
    const state = dragRef.current
    const el = ref.current
    if (!el || !state.active) return

    const dx = clientX - state.startX
    if (!state.moved) {
      if (Math.abs(dx) < DRAG_THRESHOLD_PX) return
      state.moved = true
      setIsDragging(true)

      try {
        // Capture only after threshold so a plain click still targets the card.
        el.setPointerCapture(state.pointerId)
        state.captured = true
        removeWinListeners()
      } catch {
        /* ignore */
      }
    }

    const maxScrollLeft = el.scrollWidth - el.clientWidth
    el.scrollLeft = Math.max(
      0,
      Math.min(maxScrollLeft, state.startScrollLeft - dx),
    )
  }, [removeWinListeners])

  const onWheel = useCallback((e: ReactWheelEvent<HTMLDivElement>) => {
    // 只映射纯纵向滚轮；横向 deltaX 留给浏览器，以免和惯性对打。
    if (e.deltaX !== 0 || e.deltaY === 0) return
    const el = e.currentTarget
    const maxScrollLeft = el.scrollWidth - el.clientWidth
    if (maxScrollLeft <= 0) return
    e.preventDefault()
    el.scrollLeft = Math.max(
      0,
      Math.min(maxScrollLeft, el.scrollLeft + e.deltaY),
    )
  }, [])

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      // 只主键拖；touch/pen 靠 touch-action 走原生平移。
      if (e.pointerType !== 'mouse' || e.button !== 0) return

      const target = e.target as HTMLElement | null
      if (
        target?.closest('button, a, input, textarea, select, [role="button"]')
      ) {
        return
      }

      const el = e.currentTarget
      ref.current = el
      const maxScrollLeft = el.scrollWidth - el.clientWidth
      if (maxScrollLeft <= 0) return

      removeWinListeners()

      dragRef.current = {
        pointerId: e.pointerId,
        startX: e.clientX,
        startScrollLeft: el.scrollLeft,
        moved: false,
        active: true,
        captured: false,
      }

      const pointerId = e.pointerId
      // 捕获前在 window 跟指针，离开条带也能拖完，并保证 pointerup 一定结束手势。
      const onWinMove = (ev: PointerEvent) => {
        if (ev.pointerId !== pointerId) return
        if (ev.cancelable) ev.preventDefault()
        applyDragScroll(ev.clientX)
      }
      const onWinUp = (ev: PointerEvent) => {
        if (ev.pointerId !== pointerId) return
        endDrag(ref.current, pointerId)
      }
      window.addEventListener('pointermove', onWinMove)
      window.addEventListener('pointerup', onWinUp)
      window.addEventListener('pointercancel', onWinUp)
      winListenersRef.current = () => {
        window.removeEventListener('pointermove', onWinMove)
        window.removeEventListener('pointerup', onWinUp)
        window.removeEventListener('pointercancel', onWinUp)
      }
    },
    [applyDragScroll, endDrag, removeWinListeners],
  )

  const onPointerMove = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      const state = dragRef.current
      if (!state.active || state.pointerId !== e.pointerId) return
      ref.current = e.currentTarget
      if (state.moved && e.cancelable) e.preventDefault()
      applyDragScroll(e.clientX)
    },
    [applyDragScroll],
  )

  const onPointerUp = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      ref.current = e.currentTarget
      endDrag(e.currentTarget, e.pointerId)
    },
    [endDrag],
  )

  const onPointerCancel = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      ref.current = e.currentTarget
      endDrag(e.currentTarget, e.pointerId)
    },
    [endDrag],
  )

  const onClickCapture = useCallback((e: ReactMouseEvent<HTMLDivElement>) => {
    if (!suppressClickRef.current) return
    // After a drag, suppress the synthetic click that would open a card.
    suppressClickRef.current = false
    e.preventDefault()
    e.stopPropagation()
  }, [])

  return {
    ref,
    onWheel,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
    onClickCapture,
    className: isDragging
      ? 'cursor-grabbing select-none [&_*]:!cursor-grabbing'
      : 'cursor-grab',
    style: isDragging
      ? ({ scrollSnapType: 'none' } as CSSProperties)
      : undefined,
    isDragging,
  }
}
