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
/** 触控板一次横扫会连发很多 wheel；攒够这么多算一步。 */
export const ROTATION_WHEEL_PX = 50
/**
 * 一次横扫（连同松手后的惯性）只翻一页：翻过之后，wheel 要停这么久才算下一次手势。
 * 固定时长的锁挡不住惯性——惯性能拖一秒多，锁一过又会攒够一页，一扫翻两页。
 */
export const ROTATION_WHEEL_IDLE_MS = 180
/** 手动拨过之后多久恢复自动轮换。 */
export const ROTATION_HOLD_MS = 15_000

/**
 * 已被某个轮换接管的原生事件。小组件可以嵌在会翻页的容器里（控制面板），
 * 事件冒泡到外层时外层不能再翻一次。
 */
const claimedEvents = new WeakSet<Event>()

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
  // 按住时不自动翻：触屏没有悬停暂停，手指还没滑到阈值时自动翻一页、滑动再翻一页，就成了两页。
  const [pressing, setPressing] = useState(false)
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
    setPressing(false)
    if (holdTimerRef.current) clearTimeout(holdTimerRef.current)
    holdTimerRef.current = null
  }, [active])

  const paused = active && (hovered || focused || held || pressing)

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
  const releaseRef = useRef<(() => void) | null>(null)
  useEffect(() => () => releaseRef.current?.(), [])

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
      if (claimedEvents.has(event.nativeEvent)) return
      if (ignoresGesture(event.target)) return
      claimedEvents.add(event.nativeEvent)
      gestureRef.current = {
        pointerId: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        intent: 'pending',
      }
      setPressing(true)
      // 松手可能在小组件外面，挂在 window 上才收得到。
      releaseRef.current?.()
      const pointerId = event.pointerId
      const onRelease = (ev: PointerEvent) => {
        if (ev.pointerId === pointerId) releaseRef.current?.()
      }
      releaseRef.current = () => {
        window.removeEventListener('pointerup', onRelease)
        window.removeEventListener('pointercancel', onRelease)
        releaseRef.current = null
        setPressing(false)
      }
      window.addEventListener('pointerup', onRelease)
      window.addEventListener('pointercancel', onRelease)
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
    /** 这次手势已经翻过一页。 */
    stepped: boolean
    lastAt: number
  }>({ el: null, acc: 0, stepped: false, lastAt: Number.NEGATIVE_INFINITY })
  const activeRef = useRef(active)
  activeRef.current = active
  const handleWheel = useCallback(
    (event: WheelEvent) => {
      if (!activeRef.current || event.ctrlKey) return
      if (claimedEvents.has(event)) return
      const dx = horizontalWheelDelta(event)
      if (dx === 0) return
      claimedEvents.add(event)
      event.preventDefault()
      const state = wheelRef.current
      // 停顿超过阈值才是新的一次手势；之前的余量和「已翻过」都作废。
      if (event.timeStamp - state.lastAt >= ROTATION_WHEEL_IDLE_MS) {
        state.acc = 0
        state.stepped = false
      }
      state.lastAt = event.timeStamp
      if (state.stepped) return
      state.acc += dx
      if (Math.abs(state.acc) < ROTATION_WHEEL_PX) return
      const delta = state.acc > 0 ? 1 : -1
      state.acc = 0
      state.stepped = true
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
