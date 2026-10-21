import type { PointerEvent as ReactPointerEvent, RefObject } from 'react'
import type { WidgetPopupEntry } from '../../lib/widgetPopup'
import { LuX } from '@lib/chromeStrokeIcons'
import {
  Suspense,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { createPortal } from 'react-dom'
import { useLocation } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { isExlight } from '../../hooks/useAnimationLevel'
import { useMediaQuery } from '../../hooks/useMediaQuery'
import {
  closeWidgetPopup,
  getWidgetPopupState,
  subscribeWidgetPopupState,
} from '../../lib/widgetPopup'
import { focusableWithin, trapTabKey } from '../../utils/focusTrap'
import { lockScroll } from '../../utils/scrollLock'
import { VIEWPORT_MQ } from '../../utils/viewportBands'
import { RenderErrorBoundary } from '../RenderErrorBoundary'
import { widgetDisplayLabel } from '../widgetLibraryModel'
import './WidgetPopupHost.css'

/**
 * 入场是带一点初速的弹簧：一出手就走掉一半，落位时冲过头约 2% 再回来，像被抛出来停稳。
 * 底部面板不冲过头，不然底边会离开屏幕边缘。退场不弹，干脆收走，比入场短。
 */
const ENTER_MS = 560
const SHEET_MS = 480
const EXIT_MS = 220
const FADE_MS = 140
const EASE_SPRING =
  'linear(0, 0.193, 0.3734, 0.5317, 0.6639, 0.77, 0.8518, 0.9126, 0.9558, 0.9852, 1.0039, 1.0147, 1.02, 1.0215, 1.0207, 1.0185, 1.0157, 1.0127, 1.01, 1.0075, 1.0054, 1.0037, 1.0024, 1.0015, 1)'
const EASE_SHEET =
  'linear(0, 0.2589, 0.4566, 0.6056, 0.7169, 0.799, 0.8589, 0.9023, 0.9333, 0.9552, 0.9705, 0.9811, 0.9883, 0.993, 0.9961, 0.9981, 0.9993, 1)'
const EASE_ENTER_FALLBACK = 'cubic-bezier(0.16, 1, 0.3, 1)'
const EASE_EXIT = 'cubic-bezier(0.4, 0, 1, 1)'

/** 底部面板往下拖过这么多（占高度）或甩得够快就关。 */
const SHEET_DISMISS_RATIO = 0.25
const SHEET_DISMISS_VELOCITY = 0.6

let springSupported: boolean | null = null
function enterEasing(sheet: boolean): string {
  springSupported ??=
    typeof CSS !== 'undefined' && CSS.supports('transition-timing-function', 'linear(0, 1)')
  if (!springSupported) return EASE_ENTER_FALLBACK
  return sheet ? EASE_SHEET : EASE_SPRING
}

type Phase = 'open' | 'closing' | 'stashed'

/**
 * 从卡片长出来：等比缩放，中心对齐卡片中心。不等比会把字和卡面压扁拉长，
 * 里面按尺寸排版的内容在动画那几帧也会量错。
 */
function anchorTransform(from: DOMRect, to: DOMRect): string {
  const scale = Math.min(
    from.width / Math.max(1, to.width),
    from.height / Math.max(1, to.height),
  )
  const dx = from.left + from.width / 2 - (to.left + (to.width * scale) / 2)
  const dy = from.top + from.height / 2 - (to.top + (to.height * scale) / 2)
  return `translate(${dx}px, ${dy}px) scale(${scale})`
}

function playFrame(
  frame: HTMLElement,
  direction: 'in' | 'out',
  anchor: DOMRect | null,
  rect: DOMRect,
  sheet: boolean,
  /** 收回时从这里出发：面板被拖下去一截时，从手松开的位置接着走。 */
  from = 'none',
): Animation | null {
  if (typeof frame.animate !== 'function') return null
  const reduced = isExlight()
  let hidden: Keyframe
  if (reduced) hidden = { opacity: 0 }
  else if (sheet) hidden = { opacity: 1, transform: 'translateY(100%)' }
  else if (anchor && anchor.width > 0 && rect.width > 0)
    hidden = { opacity: 0, transform: anchorTransform(anchor, rect) }
  else hidden = { opacity: 0, transform: 'scale(0.96)' }
  const shown: Keyframe = { opacity: 1, transform: direction === 'in' ? 'none' : from }
  // 透明度只在靠近卡片那一段变：整程淡入淡出时看不出是从卡片长出来的。
  const keyframes: Keyframe[] =
    reduced || sheet
      ? direction === 'in' ? [hidden, shown] : [shown, hidden]
      : direction === 'in'
        ? [hidden, { opacity: 1, offset: 0.3 }, shown]
        : [shown, { opacity: 1, offset: 0.6 }, hidden]
  return frame.animate(keyframes, {
    duration: reduced
      ? FADE_MS
      : direction === 'out' ? EXIT_MS : sheet ? SHEET_MS : ENTER_MS,
    easing: reduced || direction === 'out' ? EASE_EXIT : enterEasing(sheet),
    fill: 'none',
  })
}

/**
 * 底部面板顶上的把手：往下拖跟手走，往上拖有阻尼；松手时过线或甩得快就关，
 * 否则弹回去。只认把手，不碰内容里的滚动和 iframe。
 */
function useSheetDrag(
  frameRef: RefObject<HTMLDivElement | null>,
  dragRef: RefObject<string>,
  onDismiss: () => void,
) {
  const gesture = useRef<{
    id: number
    startY: number
    lastY: number
    lastAt: number
    velocity: number
    offset: number
  } | null>(null)

  const place = (offset: number) => {
    const frame = frameRef.current
    if (frame) frame.style.transform = offset ? `translateY(${offset}px)` : ''
  }
  const release = () => {
    gesture.current = null
    delete frameRef.current?.dataset.dragging
  }

  return {
    onPointerDown(event: ReactPointerEvent<HTMLElement>) {
      if (event.button !== 0) return
      const frame = frameRef.current
      if (!frame) return
      for (const running of frame.getAnimations()) running.cancel()
      // 捕获要等下一个指针事件才生效；手一快，第一下移动就落进内容里的跨源 iframe，
      // 被那边吞掉，之后再也收不到。拖着的时候内容不接指针。
      frame.dataset.dragging = ''
      event.currentTarget.setPointerCapture(event.pointerId)
      gesture.current = {
        id: event.pointerId,
        startY: event.clientY,
        lastY: event.clientY,
        lastAt: event.timeStamp,
        velocity: 0,
        offset: 0,
      }
    },
    onPointerMove(event: ReactPointerEvent<HTMLElement>) {
      const state = gesture.current
      if (!state || state.id !== event.pointerId) return
      const dy = event.clientY - state.startY
      const dt = Math.max(1, event.timeStamp - state.lastAt)
      state.velocity = (event.clientY - state.lastY) / dt
      state.lastY = event.clientY
      state.lastAt = event.timeStamp
      // 往上拖越拖越沉，最多露出一指宽。
      state.offset = dy >= 0 ? dy : -Math.min(24, Math.sqrt(-dy) * 2)
      place(state.offset)
    },
    onPointerUp(event: ReactPointerEvent<HTMLElement>) {
      const state = gesture.current
      if (!state || state.id !== event.pointerId) return
      release()
      const frame = frameRef.current
      if (!frame) return
      const height = frame.getBoundingClientRect().height
      const dismiss =
        state.offset > height * SHEET_DISMISS_RATIO ||
        (state.offset > 0 && state.velocity > SHEET_DISMISS_VELOCITY)
      const at = `translateY(${state.offset}px)`
      place(0)
      if (dismiss) {
        dragRef.current = at
        onDismiss()
        return
      }
      if (state.offset !== 0 && typeof frame.animate === 'function') {
        frame.animate([{ transform: at }, { transform: 'none' }], {
          duration: SHEET_MS,
          easing: enterEasing(true),
        })
      }
    },
    onPointerCancel(event: ReactPointerEvent<HTMLElement>) {
      if (gesture.current?.id !== event.pointerId) return
      release()
      place(0)
    },
  }
}

/**
 * 收回后把焦点还给谁。只有用键盘打开的才还：鼠标点开的，还回去会让浏览器把那颗按钮
 * 画成键盘焦点（原生焦点框），轮换卡也会当成有人在看而停住不翻。
 *
 * 也不还进 iframe：宿主看不见里面发生了什么，替它把焦点放回去，在应用看来就像用户
 * 刚点了它。还给外面最近一个能聚焦的容器，下一次真点进去才算数。
 */
function focusReturnTarget(element: HTMLElement | null): HTMLElement | null {
  if (!element || !isKeyboardFocused(element)) return null
  if (!(element instanceof HTMLIFrameElement)) return element
  return element.parentElement?.closest<HTMLElement>('[tabindex]') ?? null
}

function isKeyboardFocused(element: Element): boolean {
  try {
    return element.matches(':focus-visible')
  } catch {
    return false
  }
}

function WidgetPopupFrame({
  entry,
  active,
  phone,
}: {
  entry: WidgetPopupEntry
  active: boolean
  phone: boolean
}) {
  const { t, format } = useI18n()
  const frameRef = useRef<HTMLDivElement>(null)
  const restoreRef = useRef<HTMLElement | null>(null)
  /** 打开时量好的位置。开着时页面滚动锁住，收回时照用，不在那一帧再逼浏览器重排。 */
  const rectsRef = useRef<{
    anchor: DOMRect | null
    frame: DOMRect
    width: number
    height: number
  } | null>(null)
  const [phase, setPhase] = useState<Phase>(active ? 'open' : 'stashed')
  const popup = entry.widgetType.popup
  const sheet = phone && entry.size !== 'stage'
  /** 面板被拖下去时松手的位置，收回动画从这里接着走。 */
  const dragRef = useRef('none')
  const sheetDrag = useSheetDrag(frameRef, dragRef, () => closeWidgetPopup(entry.key))

  useLayoutEffect(() => {
    const frame = frameRef.current
    if (!frame) return
    if (active) {
      // 应用自己开的（焦点不在任何控件上）不算键盘打开，不还。
      const focused = document.activeElement
      restoreRef.current = focusReturnTarget(
        focused instanceof HTMLElement && focused !== document.body ? focused : null,
      )
      setPhase('open')
      // 收回动画还没放完又被打开：先停掉，不然量到的是缩到一半的框。
      for (const running of frame.getAnimations()) running.cancel()
      const anchor = entry.anchor()
      const rect = frame.getBoundingClientRect()
      rectsRef.current = { anchor, frame: rect, width: window.innerWidth, height: window.innerHeight }
      playFrame(frame, 'in', anchor, rect, sheet)
      // 收回时是 visibility:hidden，等这一帧把可见提交了才能聚焦。
      const focusTimer = requestAnimationFrame(() => {
        if (!frame.contains(document.activeElement)) frame.focus({ preventScroll: true })
      })
      return () => cancelAnimationFrame(focusTimer)
    }
    if (phase !== 'open') return
    setPhase('closing')
    // 焦点不还的时候也不能留在里面：收回的弹窗还挂着，焦点要是还在它的 iframe 里，
    // 接下来的按键会打进一个看不见的弹窗。
    const inside = document.activeElement
    if (inside instanceof HTMLElement && frame.contains(inside)) inside.blur()
    const saved = rectsRef.current
    const resized =
      !saved || saved.width !== window.innerWidth || saved.height !== window.innerHeight
    const from = dragRef.current
    dragRef.current = 'none'
    const animation = playFrame(
      frame,
      'out',
      resized ? entry.anchor() : saved.anchor,
      resized ? frame.getBoundingClientRect() : saved.frame,
      sheet,
      from,
    )
    const restore = restoreRef.current
    restoreRef.current = null
    // 焦点等收回放完再还：当场 focus 会在动画第一帧逼浏览器重算样式。中途又被打开就不还。
    const finish = (completed: boolean) => {
      setPhase((current) => (current === 'closing' ? 'stashed' : current))
      if (completed && restore?.isConnected) restore.focus({ preventScroll: true })
    }
    if (!animation) {
      finish(true)
    } else {
      animation.onfinish = () => finish(true)
      animation.oncancel = () => finish(false)
    }
    // phase 只用来判断「刚从前台收回」，不作为依赖重跑。
  }, [active])

  if (!popup) return null
  const Content = popup.component
  const label = widgetDisplayLabel(entry.widgetType, t.widgets as unknown as Record<string, unknown>)
  const visible = phase !== 'stashed'

  return (
    <div
      ref={frameRef}
      className={`widget-popup-frame widget-popup-frame--${sheet ? 'sheet' : entry.size}`}
      data-phase={phase}
      role="dialog"
      aria-modal={active ? 'true' : undefined}
      aria-label={label}
      aria-hidden={visible ? undefined : 'true'}
      inert={!active}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (trapTabKey(event.currentTarget, event.nativeEvent)) event.stopPropagation()
      }}
    >
      {active && <FocusSentinel frameRef={frameRef} edge="start" />}
      {sheet && (
        <div className="widget-popup-frame__grabber" aria-hidden="true" {...sheetDrag}>
          <span />
        </div>
      )}
      <RenderErrorBoundary
        source="widget-popup"
        resetKey={entry.widget.type}
        fallback={
          <div className="widget-popup-frame__empty">
            {format(t.errors.widgetRenderFailed, { id: entry.widget.type })}
          </div>
        }
      >
        <Suspense fallback={<div className="widget-popup-frame__empty" />}>
          <Content
            config={entry.widget}
            params={entry.params}
            active={active}
            close={() => closeWidgetPopup(entry.key)}
            onConfigChange={entry.onConfigChange}
          />
        </Suspense>
      </RenderErrorBoundary>
      {popup.chrome === 'none' ? null : (
        <button
          type="button"
          className="widget-popup-frame__close"
          aria-label={t.common.close}
          onClick={() => closeWidgetPopup(entry.key)}
        >
          <LuX aria-hidden="true" />
        </button>
      )}
      {active && <FocusSentinel frameRef={frameRef} edge="end" />}
    </div>
  )
}

/**
 * 焦点从跨源 iframe 里 Tab 出来时宿主收不到按键，浏览器会把它送到弹窗外。
 * 前后各放一个接住它的点，转回弹窗另一头，焦点就在弹窗里循环。
 */
function FocusSentinel({
  frameRef,
  edge,
}: {
  frameRef: RefObject<HTMLDivElement | null>
  edge: 'start' | 'end'
}) {
  return (
    <span
      className="widget-popup-frame__sentinel"
      tabIndex={0}
      aria-hidden="true"
      data-focus-sentinel=""
      onFocus={() => {
        const frame = frameRef.current
        if (!frame) return
        const items = focusableWithin(frame)
        const target = edge === 'start' ? items.at(-1) : items[0]
        ;(target ?? frame).focus({ preventScroll: true })
      }}
    />
  )
}

/** 全站一份。首次 openWidgetPopup 才挂。 */
export default function WidgetPopupHost() {
  const state = useSyncExternalStore(
    subscribeWidgetPopupState,
    getWidgetPopupState,
    getWidgetPopupState,
  )
  const phone = useMediaQuery(VIEWPORT_MQ.phone)
  const { pathname } = useLocation()
  const open = state.activeKey !== null

  // 换页只是注意力走开：收回，不销毁。宿主是第一次开弹窗时才挂的，挂上那一刻不算换页。
  const pathRef = useRef(pathname)
  useEffect(() => {
    if (pathRef.current === pathname) return
    pathRef.current = pathname
    closeWidgetPopup()
  }, [pathname])

  useEffect(() => {
    if (!open) return
    const release = lockScroll()
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      event.preventDefault()
      closeWidgetPopup()
    }
    document.addEventListener('keydown', onKey)
    return () => {
      release()
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  if (state.entries.length === 0) return null
  const front = state.entries.find((entry) => entry.key === state.activeKey)

  return createPortal(
    <div
      className="widget-popup-layer"
      data-open={open ? 'true' : 'false'}
      data-cover={phone && front?.size === 'stage' ? 'full' : undefined}
    >
      <div
        className="widget-popup-backdrop"
        aria-hidden="true"
        onClick={() => closeWidgetPopup()}
      />
      {state.entries.map((entry) => (
        <WidgetPopupFrame
          key={entry.key}
          entry={entry}
          active={entry.key === state.activeKey}
          phone={phone}
        />
      ))}
    </div>,
    document.body,
  )
}
