import type { RefObject } from 'react'
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

/** 退场比入场短。 */
const ENTER_MS = 340
const EXIT_MS = 220
const FADE_MS = 140
const EASE_ENTER = 'cubic-bezier(0.16, 1, 0.3, 1)'
const EASE_EXIT = 'cubic-bezier(0.4, 0, 1, 1)'

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
): Animation | null {
  if (typeof frame.animate !== 'function') return null
  const reduced = isExlight()
  let hidden: Keyframe
  if (reduced) hidden = { opacity: 0 }
  else if (sheet) hidden = { opacity: 1, transform: 'translateY(100%)' }
  else if (anchor && anchor.width > 0 && rect.width > 0)
    hidden = { opacity: 0, transform: anchorTransform(anchor, rect) }
  else hidden = { opacity: 0, transform: 'scale(0.96)' }
  const shown: Keyframe = { opacity: 1, transform: 'none' }
  // 透明度只在靠近卡片那一段变：整程淡入淡出时看不出是从卡片长出来的。
  const keyframes: Keyframe[] =
    reduced || sheet
      ? direction === 'in' ? [hidden, shown] : [shown, hidden]
      : direction === 'in'
        ? [hidden, { opacity: 1, offset: 0.3 }, shown]
        : [shown, { opacity: 1, offset: 0.6 }, hidden]
  return frame.animate(keyframes, {
    duration: reduced ? FADE_MS : direction === 'in' ? ENTER_MS : EXIT_MS,
    easing: direction === 'in' ? EASE_ENTER : EASE_EXIT,
    fill: 'none',
  })
}

/**
 * 收回后焦点不还进 iframe：宿主看不见里面发生了什么，替它把焦点放回去，
 * 在应用看来就像用户刚点了它。还给外面最近一个能聚焦的容器，下一次真点进去才算数。
 */
function focusReturnTarget(element: HTMLElement | null): HTMLElement | null {
  if (!(element instanceof HTMLIFrameElement)) return element
  return element.parentElement?.closest<HTMLElement>('[tabindex]') ?? null
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

  useLayoutEffect(() => {
    const frame = frameRef.current
    if (!frame) return
    if (active) {
      const focused = document.activeElement
      restoreRef.current = focusReturnTarget(
        focused instanceof HTMLElement && focused !== document.body
          ? focused
          : entry.returnFocus(),
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
    const saved = rectsRef.current
    const resized =
      !saved || saved.width !== window.innerWidth || saved.height !== window.innerHeight
    const animation = playFrame(
      frame,
      'out',
      resized ? entry.anchor() : saved.anchor,
      resized ? frame.getBoundingClientRect() : saved.frame,
      sheet,
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
      {popup.chrome === 'none' && !entry.widgetType.isTappWidget ? null : (
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

  return createPortal(
    <div className="widget-popup-layer" data-open={open ? 'true' : 'false'}>
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
