/**
 * 首页小组件的轮换：自动翻，也能手动拨。
 *
 * - 悬停（鼠标）或键盘聚焦时暂停；手动拨过之后一段时间内不自动翻，免得刚拨过去就被抢走。
 * - 手动：横向滑动（触摸、鼠标拖）、触控板横向滚动 / Shift+滚轮、← →、页码点。
 * - 只接管横向：竖向滑动和滚轮仍归页面滚动。
 */

import type {
  FocusEvent as ReactFocusEvent,
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
  RefCallback,
} from 'react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useVisibilityInterval } from '../../../hooks/animation'

/** 横向位移超过这么多才算翻页；之前的移动都还可能是点击或竖向滚动。 */
export const ROTATION_SWIPE_PX = 36
/** 竖向位移先超过这个、且大于横向，就让给页面滚动。 */
export const ROTATION_SCROLL_PX = 12
/** 触控板一次横扫会连发很多 wheel；攒够这么多算一步，然后锁一会儿等惯性过去。 */
export const ROTATION_WHEEL_PX = 50
export const ROTATION_WHEEL_LOCK_MS = 450
/** 手动拨过之后多久恢复自动轮换。 */
export const ROTATION_HOLD_MS = 15_000

/** 不接管手势的元素：表单控件和显式声明的区域。 */
const GESTURE_IGNORE =
  'input, textarea, select, [contenteditable="true"], [data-rotation-ignore]'

export type SwipeIntent = 'pending' | 'scroll' | 'next' | 'prev'

function ignoresGesture(target: EventTarget | null): boolean {
  const el = target as Element | null
  return typeof el?.closest === 'function' && el.closest(GESTURE_IGNORE) !== null
}

function isKeyboardFocus(target: EventTarget | null): boolean {
  const el = target as Element | null
  if (typeof el?.matches !== 'function') return false
  try {
    return el.matches(':focus-visible')
  } catch {
    return false
  }
}

/** 按一次按下以来的位移判断意图。左滑是下一页。 */
export function swipeIntent(dx: number, dy: number): SwipeIntent {
  const ax = Math.abs(dx)
  const ay = Math.abs(dy)
  if (ay >= ROTATION_SCROLL_PX && ay > ax) return 'scroll'
  if (ax >= ROTATION_SWIPE_PX && ax > ay * 1.4) return dx < 0 ? 'next' : 'prev'
  return 'pending'
}

/** 只认横向为主的 wheel；返回横向位移（px），竖向为主时返回 0。 */
export function horizontalWheelDelta(event: {
  deltaX: number
  deltaY: number
  deltaMode: number
  shiftKey: boolean
}): number {
  const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? 400 : 1
  // 鼠标 Shift+滚轮：有的浏览器已换成 deltaX，有的仍给 deltaY。
  const dx =
    event.shiftKey && event.deltaX === 0 ? event.deltaY : event.deltaX
  const dy = event.shiftKey && event.deltaX === 0 ? 0 : event.deltaY
  if (Math.abs(dx) <= Math.abs(dy)) return 0
  return dx * unit
}

export interface WidgetRotationOptions {
  /** 页数；少于 2 时什么都不接管。 */
  count: number
  /** 允许手动拨（编辑态、预览、嵌在别处时关掉）。 */
  interactive: boolean
  /** 自动轮换间隔（ms）；null 表示不自动翻。 */
  delay: number | null
  /** 自动轮换的其余条件（动画档位等）。 */
  autoplay: boolean
  /** 翻一页：1 下一页，-1 上一页。组件自己决定怎么过渡。 */
  onStep: (delta: 1 | -1) => void
  holdMs?: number
}

export interface WidgetRotation {
  /** 能手动拨（可交互且至少两页）；页码点只在这时渲染，否则键盘 Tab 会停到看不见的按钮上。 */
  active: boolean
  /** 悬停、键盘聚焦或刚手动拨过：内部的小轮换也该停。 */
  paused: boolean
  /** 页码点该不该露出来：悬停、键盘聚焦、刚手动拨过。 */
  showPager: boolean
  /** 手动翻一页。 */
  step: (delta: 1 | -1) => void
  /** 手动做了别的切换（例如点页码点）：同样暂停自动轮换。 */
  hold: () => void
  /** 挂到小组件根节点：非被动 wheel 监听要原生绑定。 */
  rootRef: RefCallback<HTMLElement>
  /** 展开到小组件根节点。 */
  rootProps: {
    onPointerEnter: (event: ReactPointerEvent<HTMLElement>) => void
    onPointerLeave: (event: ReactPointerEvent<HTMLElement>) => void
    onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void
    onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void
    onPointerUp: (event: ReactPointerEvent<HTMLElement>) => void
    onPointerCancel: (event: ReactPointerEvent<HTMLElement>) => void
    onClickCapture: (event: ReactMouseEvent<HTMLElement>) => void
    onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void
    onFocus: (event: ReactFocusEvent<HTMLElement>) => void
    onBlur: (event: ReactFocusEvent<HTMLElement>) => void
  }
  /** 根节点要加的类：横向手势交给脚本，竖向仍是页面滚动。 */
  rootClassName: string
}

export function useWidgetRotation({
  count,
  interactive,
  delay,
  autoplay,
  onStep,
  holdMs = ROTATION_HOLD_MS,
}: WidgetRotationOptions): WidgetRotation {
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [held, setHeld] = useState(false)
  const active = interactive && count > 1

  const onStepRef = useRef(onStep)
  onStepRef.current = onStep

  const holdTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const hold = useCallback(() => {
    if (holdTimerRef.current) clearTimeout(holdTimerRef.current)
    setHeld(true)
    holdTimerRef.current = setTimeout(() => {
      holdTimerRef.current = null
      setHeld(false)
    }, holdMs)
  }, [holdMs])
  useEffect(
    () => () => {
      if (holdTimerRef.current) clearTimeout(holdTimerRef.current)
    },
    [],
  )

  const step = useCallback(
    (delta: 1 | -1) => {
      onStepRef.current(delta)
      hold()
    },
    [hold],
  )

  // 不再允许手动时清掉悬停/聚焦/暂停，否则进出编辑态后会停在「暂停」。
  useEffect(() => {
    if (active) return
    setHovered(false)
    setFocused(false)
    setHeld(false)
    if (holdTimerRef.current) clearTimeout(holdTimerRef.current)
    holdTimerRef.current = null
  }, [active])

  const paused = active && (hovered || focused || held)

  // enabled 一变计时就从头算：暂停结束后整整一个间隔才翻下一页。
  useVisibilityInterval(() => onStepRef.current(1), {
    delay: delay ?? 0,
    enabled: delay != null && autoplay && count > 1 && !paused,
  })

  const gestureRef = useRef<{
    pointerId: number
    x: number
    y: number
    intent: SwipeIntent
  } | null>(null)
  const suppressClickRef = useRef(false)

  const onPointerEnter = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      // 触摸合成的 enter 不会配对 leave，会让轮换永远停着。
      if (active && event.pointerType === 'mouse') setHovered(true)
    },
    [active],
  )
  const onPointerLeave = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      if (event.pointerType === 'mouse') setHovered(false)
    },
    [],
  )

  const onPointerDown = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      suppressClickRef.current = false
      gestureRef.current = null
      if (!active || !event.isPrimary || event.button !== 0) return
      if (ignoresGesture(event.target)) return
      gestureRef.current = {
        pointerId: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        intent: 'pending',
      }
    },
    [active],
  )

  const onPointerMove = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      const gesture = gestureRef.current
      if (!gesture || gesture.pointerId !== event.pointerId) return
      if (gesture.intent !== 'pending') return
      const intent = swipeIntent(
        event.clientX - gesture.x,
        event.clientY - gesture.y,
      )
      if (intent === 'pending') return
      gesture.intent = intent
      if (intent === 'scroll') return
      // 这次按下已经是滑动：松手时的点击不能再打开链接/跳页。
      suppressClickRef.current = true
      window.getSelection?.()?.removeAllRanges()
      step(intent === 'next' ? 1 : -1)
    },
    [step],
  )

  const endGesture = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (gestureRef.current?.pointerId === event.pointerId) {
      gestureRef.current = null
    }
  }, [])

  const onClickCapture = useCallback((event: ReactMouseEvent<HTMLElement>) => {
    if (!suppressClickRef.current) return
    suppressClickRef.current = false
    event.preventDefault()
    event.stopPropagation()
  }, [])

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLElement>) => {
      if (!active || event.defaultPrevented) return
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
        return
      }
      if (ignoresGesture(event.target)) return
      event.preventDefault()
      step(event.key === 'ArrowRight' ? 1 : -1)
    },
    [active, step],
  )

  // 鼠标点进去也会聚焦；只有键盘聚焦（:focus-visible）才算「在看」。
  const onFocus = useCallback(
    (event: ReactFocusEvent<HTMLElement>) => {
      if (!active) return
      if (isKeyboardFocus(event.target)) setFocused(true)
    },
    [active],
  )
  const onBlur = useCallback((event: ReactFocusEvent<HTMLElement>) => {
    const next = event.relatedTarget as Node | null
    if (next && event.currentTarget.contains(next)) return
    setFocused(false)
  }, [])

  // wheel 要能 preventDefault（拦住 macOS 横扫后退），React 的 onWheel 是被动的。
  const wheelRef = useRef<{
    el: HTMLElement | null
    acc: number
    lockedUntil: number
  }>({ el: null, acc: 0, lockedUntil: 0 })
  const activeRef = useRef(active)
  activeRef.current = active
  const handleWheel = useCallback(
    (event: WheelEvent) => {
      if (!activeRef.current || event.ctrlKey) return
      const dx = horizontalWheelDelta(event)
      if (dx === 0) return
      event.preventDefault()
      const state = wheelRef.current
      if (event.timeStamp < state.lockedUntil) return
      state.acc += dx
      if (Math.abs(state.acc) < ROTATION_WHEEL_PX) return
      const delta = state.acc > 0 ? 1 : -1
      state.acc = 0
      state.lockedUntil = event.timeStamp + ROTATION_WHEEL_LOCK_MS
      step(delta)
    },
    [step],
  )
  const rootRef = useCallback(
    (el: HTMLElement | null) => {
      const state = wheelRef.current
      if (state.el === el) return
      state.el?.removeEventListener('wheel', handleWheel)
      state.el = el
      state.acc = 0
      el?.addEventListener('wheel', handleWheel, { passive: false })
    },
    [handleWheel],
  )

  return {
    active,
    paused,
    showPager: active && (hovered || focused || held),
    step,
    hold,
    rootRef,
    rootProps: {
      onPointerEnter,
      onPointerLeave,
      onPointerDown,
      onPointerMove,
      onPointerUp: endGesture,
      onPointerCancel: endGesture,
      onClickCapture,
      onKeyDown,
      onFocus,
      onBlur,
    },
    rootClassName: active ? 'touch-pan-y' : '',
  }
}
