import type { PointerEvent as ReactPointerEvent, RefObject } from 'react'
import type { WidgetPopupAnchor, WidgetPopupEntry } from '../../lib/widgetPopup'
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
import { cloneCardFace, syncCardAnimations } from './cardStandIn'
import './WidgetPopupHost.css'

/**
 * 入场是带一点初速的弹簧：一出手就走掉一半，落位时冲过头约 2% 再回来，像被抛出来停稳。
 * 底部面板不冲过头，不然底边会离开屏幕边缘。
 * 收回到卡片是反过来的一程：一下子缩走大半，越靠近卡片越慢，轻轻落进去，
 * 最后一段才淡掉，露出底下的卡片。底部面板收回是滑出屏幕，越走越快。
 */
const ENTER_MS = 560
const SHEET_MS = 480
const LAND_MS = 360
const EXIT_MS = 220
const FADE_MS = 140
const EASE_SPRING =
  'linear(0, 0.193, 0.3734, 0.5317, 0.6639, 0.77, 0.8518, 0.9126, 0.9558, 0.9852, 1.0039, 1.0147, 1.02, 1.0215, 1.0207, 1.0185, 1.0157, 1.0127, 1.01, 1.0075, 1.0054, 1.0037, 1.0024, 1.0015, 1)'
const EASE_SHEET =
  'linear(0, 0.2589, 0.4566, 0.6056, 0.7169, 0.799, 0.8589, 0.9023, 0.9333, 0.9552, 0.9705, 0.9811, 0.9883, 0.993, 0.9961, 0.9981, 0.9993, 1)'
const EASE_ENTER_FALLBACK = 'cubic-bezier(0.16, 1, 0.3, 1)'
const EASE_LAND = 'cubic-bezier(0.32, 0.72, 0, 1)'
const EASE_EXIT = 'cubic-bezier(0.4, 0, 1, 1)'
/**
 * 收回途中（按时间）把框里的内容从弹窗换成卡片，三层各走各的：
 * 弹窗的内容先淡出，卡片副本随后淡入，两段有一截重叠，是看得见的交替；
 * 弹窗的底色等卡片完全显出来以后才退。卡片常是半透明的毛玻璃，底色一直垫着，
 * 交替那段透不出别的东西。形状最后那点贴合时框里已经只有卡片，落地不留重影。
 * 做不出副本时弹窗自己在 FADE 那一段整个淡掉。
 */
const CONTENT_OUT_FROM = 0.18
const CONTENT_OUT_TO = 0.5
const STAND_IN_FROM = 0.32
const STAND_IN_TO = 0.66
const SURFACE_OUT_FROM = 0.6
const SURFACE_OUT_TO = 0.8
const FADE_FROM = 0.5
const FADE_TO = 0.9
/**
 * 弹窗的投影伸出边框外这么远（上、右、下、左）。收回时裁切从投影外沿开始，
 * 第一帧不会把投影一刀切掉。
 */
const SHADOW_REACH = [30, 70, 110, 70] as const

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
function anchorTransform(from: DOMRect, to: DOMRect, cover = false): { transform: string; scale: number } {
  const fit = cover ? Math.max : Math.min
  const scale = fit(from.width / Math.max(1, to.width), from.height / Math.max(1, to.height))
  const dx = from.left + from.width / 2 - (to.left + (to.width * scale) / 2)
  const dy = from.top + from.height / 2 - (to.top + (to.height * scale) / 2)
  return { transform: `translate(${dx}px, ${dy}px) scale(${scale})`, scale }
}

/**
 * 落回卡片：等比缩到盖住卡片，多出来的部分裁掉，圆角裁成卡片的圆角，
 * 最后一帧和卡片看得见的那一面严丝合缝。裁切量按缩放前的尺寸算。
 */
function landing(anchor: WidgetPopupAnchor, rect: DOMRect, radius: number, full: boolean) {
  const { transform, scale } = anchorTransform(anchor.rect, rect, true)
  const x = Math.max(0, (rect.width - anchor.rect.width / scale) / 2)
  const y = Math.max(0, (rect.height - anchor.rect.height / scale) / 2)
  const [top, right, bottom, left] = full ? [0, 0, 0, 0] : SHADOW_REACH
  const frame: Keyframe[] = [
    { transform: 'none', clipPath: `inset(${-top}px ${-right}px ${-bottom}px ${-left}px round ${radius}px)` },
    { transform, clipPath: `inset(${y}px ${x}px ${y}px ${x}px round ${anchor.radius / scale}px)` },
  ]
  // 卡片副本就是框里裁出来的那一块：起点放大到弹窗的比例，和框同一条曲线落到卡片上。
  // 位移和缩放都随进度线性变，两份动画同一帧开始、同一条缓动，一直对得上。
  const standIn: Keyframe[] = [
    { transform: `translate(${rect.left + x}px, ${rect.top + y}px) scale(${1 / scale})` },
    { transform: `translate(${anchor.rect.left}px, ${anchor.rect.top}px) scale(1)` },
  ]
  return { frame, standIn }
}

/** 按时间的一段淡入淡出，不跟形状的缓动：形状末段走得很慢，跟着它会从一半就开始发虚。 */
function fadeBetween(from: number, to: number, appear: boolean): Keyframe[] {
  const [start, end] = appear ? [0, 1] : [1, 0]
  return [{ opacity: start }, { opacity: start, offset: from }, { opacity: end, offset: to }, { opacity: end }]
}

function playFrame(
  frame: HTMLElement,
  direction: 'in' | 'out',
  anchor: WidgetPopupAnchor | null,
  rect: DOMRect,
  sheet: boolean,
  /** 收回时从这里出发：面板被拖下去一截时，从手松开的位置接着走。 */
  from = 'none',
  /** 弹窗自己的圆角，和它是不是铺满整屏（铺满时没有投影要让）。 */
  shape: { radius: number; full: boolean } = { radius: 0, full: false },
  /** 收回快落地时先换上的卡片副本，已经挂好。 */
  standIn: HTMLElement | null = null,
  /** 除了返回的那一份，这里放进其余停在最后一帧的动画，收好时一起清掉。 */
  held: Animation[] = [],
): Animation | null {
  if (typeof frame.animate !== 'function') return null
  const reduced = isExlight()
  if (!reduced && !sheet && direction === 'out' && anchor && anchor.rect.width > 0 && rect.width > 0) {
    const path = landing(anchor, rect, shape.radius, shape.full)
    // 收回停在最后一帧：动画一结束样式就回到原状，而「已收回」要等 React 下一次提交，
    // 中间那一帧整个弹窗会原样闪出来。藏好之后再由组件清掉。
    const timing: KeyframeAnimationOptions = { duration: LAND_MS, easing: EASE_LAND, fill: 'forwards' }
    const fade: KeyframeAnimationOptions = { duration: LAND_MS, easing: 'linear', fill: 'forwards' }
    const motion = frame.animate(path.frame, timing)
    if (standIn) {
      // 内容是框里的每个子元素（舞台、关闭键…），底色是框自己。
      for (const child of frame.children) {
        held.push(child.animate(fadeBetween(CONTENT_OUT_FROM, CONTENT_OUT_TO, false), fade))
      }
      standIn.animate(path.standIn, timing)
      standIn.animate(fadeBetween(STAND_IN_FROM, STAND_IN_TO, true), fade)
      frame.animate(fadeBetween(SURFACE_OUT_FROM, SURFACE_OUT_TO, false), fade)
    } else {
      frame.animate(fadeBetween(FADE_FROM, FADE_TO, false), fade)
    }
    return motion
  }
  let hidden: Keyframe
  if (reduced) hidden = { opacity: 0 }
  else if (sheet) hidden = { opacity: 1, transform: 'translateY(100%)' }
  else if (anchor && anchor.rect.width > 0 && rect.width > 0)
    hidden = { opacity: 0, transform: anchorTransform(anchor.rect, rect).transform }
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
    // 收回同样停在最后一帧，理由见上。
    fill: direction === 'out' ? 'forwards' : 'none',
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
    anchor: WidgetPopupAnchor | null
    frame: DOMRect
    radius: number
    width: number
    height: number
  } | null>(null)
  const [phase, setPhase] = useState<Phase>(active ? 'open' : 'stashed')
  const popup = entry.widgetType.popup
  const sheet = phone && entry.size !== 'stage'
  /** 面板被拖下去时松手的位置，收回动画从这里接着走。 */
  const dragRef = useRef('none')
  /** 收回时停在框的子元素上的淡出，收好、重开时清掉（框自己身上的另外清）。 */
  const heldRef = useRef<Animation[]>([])
  const releaseHeld = () => {
    for (const animation of heldRef.current) animation.cancel()
    heldRef.current = []
  }
  /** 收回时叠在弹窗上的卡片副本，收好、重开或卸载时拿掉，真卡片同时露回来。 */
  const standInRef = useRef<{ copy: HTMLElement; face: HTMLElement; visibility: string } | null>(null)
  const dropStandIn = () => {
    const held = standInRef.current
    if (!held) return
    held.copy.remove()
    held.face.style.visibility = held.visibility
    standInRef.current = null
  }
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
      releaseHeld()
      dropStandIn()
      const anchor = entry.anchor()
      const rect = frame.getBoundingClientRect()
      const radius = Number.parseFloat(getComputedStyle(frame).borderTopLeftRadius) || 0
      rectsRef.current = { anchor, frame: rect, radius, width: window.innerWidth, height: window.innerHeight }
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
    const anchor = resized ? entry.anchor() : saved.anchor
    const standIn = !sheet && !isExlight() && anchor?.face?.isConnected ? cloneCardFace(anchor.face) : null
    if (standIn && anchor?.face) {
      const face = anchor.face
      document.body.append(standIn)
      syncCardAnimations(face, standIn)
      // 副本是半透明的毛玻璃，压在真卡片上会把它也糊进去，落地时偏白。
      // 真卡片这段时间一直被弹窗盖着，先藏起来看不出；副本拿掉的同一帧再露出来。
      standInRef.current = { copy: standIn, face, visibility: face.style.visibility }
      face.style.visibility = 'hidden'
    }
    const animation = playFrame(
      frame,
      'out',
      anchor,
      resized ? frame.getBoundingClientRect() : saved.frame,
      sheet,
      from,
      { radius: saved?.radius ?? 0, full: phone && entry.size === 'stage' },
      standIn,
      heldRef.current,
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

  // 「已收回」的样式已经提交、这一帧还没画：现在清掉停在最后一帧的收回动画，看不见。
  // 卡片副本同一帧拿掉，底下的真卡片和它长得一样，换过去看不出来。
  useLayoutEffect(() => {
    if (phase !== 'stashed') return
    for (const held of frameRef.current?.getAnimations() ?? []) {
      held.onfinish = null
      held.oncancel = null
      held.cancel()
    }
    releaseHeld()
    dropStandIn()
  }, [phase])

  useEffect(() => dropStandIn, [])

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
            // 收回动画放完才告诉内容停下：缩的那几帧里暂停键换图标、背景光停住跳位，看着就是一闪。
            active={active || phase !== 'stashed'}
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
