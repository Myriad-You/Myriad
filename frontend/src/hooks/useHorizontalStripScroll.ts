import type {
  CSSProperties,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
  RefCallback,
} from 'react'
import { useCallback, useEffect, useRef, useState } from 'react'

const DRAG_THRESHOLD_PX = 6

/**
 * 吸附条上滚轮换卡：
 * - 停顿这么久之后的第一个 wheel 是新手势，走一张；
 * - 同一手势里幅度在衰减的是松手后的惯性，吞掉（固定时长的锁挡不住一秒多的惯性）；
 * - 幅度没衰减的是又一格（鼠标连续滚），离上一步够久就再走一张；
 * - 走到头就放行给页面：条带占满整宽，竖着滚页面一定会经过它，不能在这里卡住。
 */
const SNAP_GESTURE_IDLE_MS = 180
const SNAP_REPEAT_MS = 150

export interface HorizontalStripScrollBind {
  ref: RefCallback<HTMLDivElement>
  onPointerDown: (e: ReactPointerEvent<HTMLDivElement>) => void
  onPointerMove: (e: ReactPointerEvent<HTMLDivElement>) => void
  onPointerUp: (e: ReactPointerEvent<HTMLDivElement>) => void
  onPointerCancel: (e: ReactPointerEvent<HTMLDivElement>) => void
  onClickCapture: (e: ReactMouseEvent<HTMLDivElement>) => void
  onPointerEnter: (e: ReactPointerEvent<HTMLDivElement>) => void
  className: string
  style: CSSProperties | undefined
  isDragging: boolean
}

/** 原生 overflow-x 不管普通鼠标滚轮；桌面需要把纵向滚轮映射成横向，并支持拖拽。 */
export function useHorizontalStripScroll(): HorizontalStripScrollBind {
  const ref = useRef<HTMLDivElement | null>(null)
  const [isDragging, setIsDragging] = useState(false)
  const [canScroll, setCanScroll] = useState(false)
  const observersRef = useRef<(() => void) | null>(null)
  const snapGestureRef = useRef({
    lastAt: Number.NEGATIVE_INFINITY,
    lastMagnitude: 0,
    lastStepAt: Number.NEGATIVE_INFINITY,
  })

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

  const onWheel = useCallback((e: WheelEvent) => {
    // 只映射纯纵向滚轮；横向 deltaX 留给浏览器，以免和惯性对打。
    if (e.ctrlKey || e.deltaX !== 0 || e.deltaY === 0 || !e.cancelable) return
    const el = e.currentTarget as HTMLDivElement
    const maxScrollLeft = el.scrollWidth - el.clientWidth
    if (maxScrollLeft <= 0) return
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? el.clientWidth : 1
    const delta = e.deltaY * unit
    const snapType = window.getComputedStyle(el).scrollSnapType
    if (snapType && snapType !== 'none') {
      // 直接写 scrollLeft 是「落点」滚动：一格滚轮不到半张卡，mandatory 吸附会弹回原位。
      // scrollBy 是「方向」滚动，浏览器吸附到该方向的下一张卡。
      const gesture = snapGestureRef.current
      const magnitude = Math.abs(delta)
      const fresh = e.timeStamp - gesture.lastAt >= SNAP_GESTURE_IDLE_MS
      const decaying = !fresh && magnitude < gesture.lastMagnitude
      gesture.lastAt = e.timeStamp
      gesture.lastMagnitude = magnitude
      const room = delta > 0 ? maxScrollLeft - el.scrollLeft : el.scrollLeft
      if (room <= 1) return
      e.preventDefault()
      if (!fresh && (decaying || e.timeStamp - gesture.lastStepAt < SNAP_REPEAT_MS)) {
        return
      }
      gesture.lastStepAt = e.timeStamp
      el.scrollBy({ left: delta, behavior: 'smooth' })
      return
    }
    const next = Math.max(
      0,
      Math.min(maxScrollLeft, el.scrollLeft + delta),
    )
    if (next === el.scrollLeft) return
    e.preventDefault()
    el.scrollLeft = next
  }, [])

  // The strip mounts conditionally. Bind to the actual node, not a mount-only
  // effect, and avoid React's passive delegated wheel listener.
  const setStripRef = useCallback((el: HTMLDivElement | null) => {
    ref.current?.removeEventListener('wheel', onWheel)
    observersRef.current?.()
    observersRef.current = null
    ref.current = el
    el?.addEventListener('wheel', onWheel, { passive: false })
    if (!el) return
    // 卡片增减、窗口变宽变窄都会改变能不能滚；指针停在条上时光标也要跟着变。
    const update = () => setCanScroll(el.scrollWidth - el.clientWidth > 0)
    update()
    const resize =
      typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(update)
    resize?.observe(el)
    const mutation =
      typeof MutationObserver === 'undefined' ? null : new MutationObserver(update)
    mutation?.observe(el, { childList: true })
    observersRef.current = () => {
      resize?.disconnect()
      mutation?.disconnect()
    }
  }, [onWheel])

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

  // 光标只在进入时判断：可滚宽度随卡片数和视口变化，进入那一刻的几何就是用户看到的。
  const onPointerEnter = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget
    setCanScroll(el.scrollWidth - el.clientWidth > 0)
  }, [])

  const onClickCapture = useCallback((e: ReactMouseEvent<HTMLDivElement>) => {
    if (!suppressClickRef.current) return
    // After a drag, suppress the synthetic click that would open a card.
    suppressClickRef.current = false
    e.preventDefault()
    e.stopPropagation()
  }, [])

  return {
    ref: setStripRef,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
    onClickCapture,
    onPointerEnter,
    className: isDragging
      ? 'cursor-grabbing select-none [&_*]:!cursor-grabbing'
      : canScroll
        ? 'cursor-grab'
        : '',
    style: isDragging
      ? ({ scrollSnapType: 'none' } as CSSProperties)
      : undefined,
    isDragging,
  }
}
