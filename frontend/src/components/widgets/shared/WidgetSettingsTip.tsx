import type { MouseEvent as ReactMouseEvent, ReactNode, RefObject } from 'react'
import type { GuidePlacement } from '../../settings/settingTitleGuideLogic'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../../contexts/I18nContext'
import {
  SETTINGS_DURATION,
  SETTINGS_DURATION_MS,
  SETTINGS_EASE,
} from '../../settings/motion'
import {
  computeGuidePosition,

} from '../../settings/settingTitleGuideLogic'
import '../../settings/settings-motion.css'
import './WidgetSettingsTip.css'

const DEFAULT_WIDTH = 300
const DEFAULT_HEIGHT = 320
const DEFAULT_PAD = 12

const exclusiveTip: {
  id: object | null
  close: (() => void) | null
} = {
  id: null,
  close: null,
}

// 锚齿轮，不是整张卡片；与 .widget-longpress-hint 的 top/left/size 对齐。
const HINT_INSET = 6.4
const HINT_SIZE = 24.8

function originForPlacement(placement: GuidePlacement): string {
  if (placement === 'top') return 'center bottom'
  if (placement === 'bottom') return 'center top'
  if (placement === 'left') return 'right center'
  return 'left center'
}

function shiftForPlacement(placement: GuidePlacement): { x: number; y: number } {
  if (placement === 'top') return { x: 0, y: 10 }
  if (placement === 'bottom') return { x: 0, y: -10 }
  if (placement === 'left') return { x: 12, y: 0 }
  return { x: -12, y: 0 }
}

function hintTriggerRect(widget: {
  top: number
  left: number
}): {
  top: number
  left: number
  right: number
  bottom: number
  width: number
  height: number
} {
  const left = widget.left + HINT_INSET
  const top = widget.top + HINT_INSET
  return {
    top,
    left,
    width: HINT_SIZE,
    height: HINT_SIZE,
    right: left + HINT_SIZE,
    bottom: top + HINT_SIZE,
  }
}

export function widgetSettingsTipPosition(
  anchor: DOMRect | null,
  width = DEFAULT_WIDTH,
  height = DEFAULT_HEIGHT,
  pad = DEFAULT_PAD,
): { top: number; left: number; origin: string; placement: GuidePlacement } {
  if (!anchor) {
    return { top: 0, left: 0, origin: 'left center', placement: 'right' }
  }
  const coords = computeGuidePosition(
    hintTriggerRect(anchor),
    width,
    height,
    window.innerWidth,
    window.innerHeight,
    pad,
  )
  return {
    top: coords.top,
    left: coords.left,
    origin: originForPlacement(coords.placement),
    placement: coords.placement,
  }
}

export interface WidgetSettingsTipProps {
  open: boolean
  anchor: DOMRect | null
  title: string
  subtitle?: string
  width?: number
  height?: number
  onClose: () => void
  ignoreRef?: RefObject<HTMLElement | null>
  children: ReactNode
}

export function WidgetSettingsTip({
  open,
  anchor,
  title,
  subtitle,
  width = DEFAULT_WIDTH,
  height = DEFAULT_HEIGHT,
  onClose,
  ignoreRef,
  children,
}: WidgetSettingsTipProps) {
  const { t } = useI18n()
  const tipRef = useRef<HTMLDivElement>(null)
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose
  const [present, setPresent] = useState(open)
  const [measured, setMeasured] = useState({ w: width, h: height })
  const [ready, setReady] = useState(false)
  const [liveAnchor, setLiveAnchor] = useState<DOMRect | null>(anchor)
  const position = useMemo(
    () => widgetSettingsTipPosition(liveAnchor ?? anchor, measured.w, measured.h),
    [anchor, liveAnchor, measured.h, measured.w],
  )
  const shift = shiftForPlacement(position.placement)

  useEffect(() => {
    if (!open) return
    const id = {}
    const previous = exclusiveTip.close
    exclusiveTip.id = id
    exclusiveTip.close = () => {
      onCloseRef.current()
    }
    previous?.()
    return () => {
      if (exclusiveTip.id === id) {
        exclusiveTip.id = null
        exclusiveTip.close = null
      }
    }
  }, [open])

  useLayoutEffect(() => {
    if (!open) {
      setLiveAnchor(anchor)
      return
    }
    const sync = () => {
      const node = ignoreRef?.current
      setLiveAnchor(node ? node.getBoundingClientRect() : anchor)
    }
    sync()
    window.addEventListener('resize', sync)
    return () => window.removeEventListener('resize', sync)
  }, [open, anchor, ignoreRef])

  useLayoutEffect(() => {
    if (!open) {
      setMeasured({ w: width, h: height })
      setReady(false)
      return
    }
    const node = tipRef.current
    if (!node || typeof ResizeObserver === 'undefined') return
    let shown = false
    const apply = () => {
      const nextW = node.offsetWidth
      const nextH = node.offsetHeight
      if (nextW < 1 || nextH < 1) return
      setMeasured((prev) =>
        Math.abs(prev.w - nextW) < 1 && Math.abs(prev.h - nextH) < 1
          ? prev
          : { w: nextW, h: nextH },
      )
      if (!shown) {
        shown = true
        requestAnimationFrame(() => setReady(true))
      }
    }
    apply()
    const observer = new ResizeObserver(apply)
    observer.observe(node)
    return () => observer.disconnect()
  }, [open, width, height])

  useEffect(() => {
    if (open) {
      setPresent(true)
      return
    }
    const timer = window.setTimeout(setPresent, SETTINGS_DURATION_MS.base, false)
    return () => window.clearTimeout(timer)
  }, [open])

  useEffect(() => {
    if (!open) return
    const onDoc = (event: MouseEvent) => {
      const target = event.target as Node
      if (tipRef.current?.contains(target)) return
      if (ignoreRef?.current?.contains(target)) return
      onClose()
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }
    const timer = window.setTimeout(() => {
      document.addEventListener('mousedown', onDoc)
      document.addEventListener('keydown', onKey)
    }, 100)
    return () => {
      window.clearTimeout(timer)
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey)
    }
  }, [open, onClose, ignoreRef])

  if (!present && !open) return null

  return createPortal(
    <AnimatePresence mode="sync">
    {open ? (
    <motion.div
      key="widget-settings-tip"
      ref={tipRef}
      className="widget-settings-tip"
      initial={{ opacity: 0, scale: 0.94, x: shift.x, y: shift.y }}
      animate={
        ready
          ? { opacity: 1, scale: 1, x: 0, y: 0 }
          : { opacity: 0, scale: 0.96, x: shift.x, y: shift.y }
      }
      exit={{
        opacity: 0,
        scale: 0.96,
        x: shift.x * 0.55,
        y: shift.y * 0.55,
        transition: {
          duration: SETTINGS_DURATION.fast,
          ease: SETTINGS_EASE.exit,
        },
      }}
      transition={{
        duration: SETTINGS_DURATION.base,
        ease: SETTINGS_EASE.enter,
      }}
      style={{
        top: position.top,
        left: position.left,
        width,
        transformOrigin: position.origin,
      }}
      onMouseDown={(event: ReactMouseEvent<HTMLDivElement>) =>
        event.stopPropagation()
      }
    >
      <div className="widget-settings-tip__head">
        <div className="widget-settings-tip__heading">
          <span className="widget-settings-tip__title">{title}</span>
          {subtitle ? (
            <p className="widget-settings-tip__subtitle">{subtitle}</p>
          ) : null}
        </div>
        <button
          type="button"
          className="widget-settings-tip__done"
          onClick={onClose}
        >
          {t.common.done}
        </button>
      </div>
      {children}
    </motion.div>
    ) : null}
    </AnimatePresence>,
    document.body,
  )
}

export function WidgetSettingsSection({
  label,
  children,
}: {
  label?: string
  children: ReactNode
}) {
  return (
    <section className="widget-settings-tip__section">
      {label ? (
        <h3 className="widget-settings-tip__section-label">{label}</h3>
      ) : null}
      {children}
    </section>
  )
}

export function WidgetSettingsChoices({
  label,
  row,
  children,
}: {
  label?: string
  row?: boolean
  children: ReactNode
}) {
  return (
    <div
      className={`widget-settings-tip__choices${row ? ' is-row' : ''}`}
      role="radiogroup"
      aria-label={label}
    >
      {children}
    </div>
  )
}

export function WidgetSettingsChoice({
  selected,
  onClick,
  label,
  hint,
  children,
}: {
  selected: boolean
  onClick: () => void
  label?: string
  hint?: string
  children?: ReactNode
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      className={`widget-settings-tip__choice${selected ? ' is-on' : ''}`}
      onClick={onClick}
    >
      {children ?? (
        <>
          {label ? (
            <span className="widget-settings-tip__choice-label">{label}</span>
          ) : null}
          {hint ? (
            <span className="widget-settings-tip__choice-hint">{hint}</span>
          ) : null}
        </>
      )}
    </button>
  )
}

export function WidgetSettingsAction({
  onClick,
  label,
  hint,
  disabled,
}: {
  onClick: () => void
  label: string
  hint?: string
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      className="widget-settings-tip__action"
      onClick={onClick}
      disabled={disabled}
    >
      <span className="widget-settings-tip__choice-label">{label}</span>
      {hint ? (
        <span className="widget-settings-tip__choice-hint">{hint}</span>
      ) : null}
    </button>
  )
}
