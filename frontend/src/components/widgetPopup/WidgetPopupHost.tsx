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
  dropWidgetPopups,
  getWidgetPopupState,
  subscribeWidgetPopupState,
} from '../../lib/widgetPopup'
import { trapTabKey } from '../../utils/focusTrap'
import { lockScroll } from '../../utils/scrollLock'
import { useTappSubject } from '../../utils/tappSubject'
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

function anchorTransform(from: DOMRect, to: DOMRect): string {
  const sx = from.width / Math.max(1, to.width)
  const sy = from.height / Math.max(1, to.height)
  const dx = from.left - to.left
  const dy = from.top - to.top
  return `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`
}

function playFrame(
  frame: HTMLElement,
  direction: 'in' | 'out',
  anchor: DOMRect | null,
  sheet: boolean,
): Animation | null {
  if (typeof frame.animate !== 'function') return null
  const rect = frame.getBoundingClientRect()
  const reduced = isExlight()
  let hidden: Keyframe
  if (reduced) hidden = { opacity: 0 }
  else if (sheet) hidden = { opacity: 1, transform: 'translateY(100%)' }
  else if (anchor && anchor.width > 0 && rect.width > 0)
    hidden = { opacity: 0, transform: anchorTransform(anchor, rect), borderRadius: '24px' }
  else hidden = { opacity: 0, transform: 'scale(0.96)' }
  const shown: Keyframe = { opacity: 1, transform: 'none' }
  const keyframes = direction === 'in' ? [hidden, shown] : [shown, hidden]
  return frame.animate(keyframes, {
    duration: reduced ? FADE_MS : direction === 'in' ? ENTER_MS : EXIT_MS,
    easing: direction === 'in' ? EASE_ENTER : EASE_EXIT,
    fill: 'none',
  })
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
  const [phase, setPhase] = useState<Phase>(active ? 'open' : 'stashed')
  const popup = entry.widgetType.popup
  const sheet = phone && entry.size !== 'stage'

  useLayoutEffect(() => {
    const frame = frameRef.current
    if (!frame) return
    if (active) {
      const focused = document.activeElement
      restoreRef.current =
        focused instanceof HTMLElement && focused !== document.body
          ? focused
          : entry.returnFocus()
      setPhase('open')
      playFrame(frame, 'in', entry.anchor(), sheet)
      // 暂存时是 visibility:hidden，等这一帧把可见提交了才能聚焦。
      const focusTimer = requestAnimationFrame(() => {
        if (!frame.contains(document.activeElement)) frame.focus({ preventScroll: true })
      })
      return () => cancelAnimationFrame(focusTimer)
    }
    if (phase !== 'open') return
    setPhase('closing')
    const animation = playFrame(frame, 'out', entry.anchor(), sheet)
    const finish = () => setPhase((current) => (current === 'closing' ? 'stashed' : current))
    if (!animation) {
      finish()
    } else {
      animation.onfinish = finish
      animation.oncancel = finish
    }
    const restore = restoreRef.current
    restoreRef.current = null
    if (restore?.isConnected) restore.focus({ preventScroll: true })
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
    </div>
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
  const { epoch } = useTappSubject()
  const { pathname } = useLocation()
  const open = state.activeKey !== null

  // 换身份：授权前提没了，连暂存一起销毁。
  const epochRef = useRef(epoch)
  useEffect(() => {
    if (epochRef.current === epoch) return
    epochRef.current = epoch
    dropWidgetPopups()
  }, [epoch])

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
