import type { ReactNode } from 'react'
import type { GuidePlacement } from './settingTitleGuideLogic'
import { FaTimes, LuGripVertical, LuPin } from '@lib/icons'
import React, {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../contexts/I18nContext'
import { useSettingsHelp } from './SettingsHelpContext'
import {
  applyGuideDragDelta,
  clampPanelToViewport,
  computeGuidePosition,
  GUIDE_VIEWPORT_PAD,
  isGuideAnchorVisible,
  shouldAllowGuideClose,
  shouldToggleCloseGuide,
} from './settingTitleGuideLogic'
import './SettingTitleGuideEntry.css'

export interface SettingTitleGuideTriggerApi {
  open: boolean
  closing: boolean
  toggle: (e?: React.MouseEvent) => void
  panelId: string
  mounted: boolean
  ariaLabel: string
  pinned: boolean
}

export interface SettingTitleGuideEntryProps {
  title: string
  guide?: ReactNode
  className?: string
  requireShowDetails?: boolean
  openLabel?: string
  closeLabel?: string
  panelClassName?: string
  renderTrigger?: (api: SettingTitleGuideTriggerApi) => ReactNode
}

const PANEL_MAX_W = 36 * 16 // 36rem
const LERP = 0.12
/** stop rAF when close enough */
const SNAP_EPS = 0.45
const EXIT_MS = 260

type FloatPhase = 'closed' | 'open' | 'closing'

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function resolveVisibilityTarget(trigger: HTMLElement): HTMLElement {
  const anchor = trigger.closest(
    '.has-guide-anchor, .setting-item, .setting-group, .section-header-text',
  )
  return (anchor as HTMLElement | null) ?? trigger
}

function domRectToGuideRect(rect: DOMRect) {
  return {
    top: rect.top,
    left: rect.left,
    right: rect.right,
    bottom: rect.bottom,
    width: rect.width,
    height: rect.height,
  }
}

function isAnchorVisible(el: HTMLElement): boolean {
  return isGuideAnchorVisible(
    domRectToGuideRect(el.getBoundingClientRect()),
    window.innerWidth,
    window.innerHeight,
  )
}

export const SettingTitleGuideEntry: React.FC<SettingTitleGuideEntryProps> = ({
  title,
  guide,
  className = '',
  requireShowDetails = true,
  openLabel,
  closeLabel,
  panelClassName = '',
  renderTrigger,
}) => {
  const { t, format } = useI18n()
  const help = useSettingsHelp()
  const panelId = useId()
  const triggerRef = useRef<HTMLElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  const [phase, setPhase] = useState<FloatPhase>('closed')
  const [ready, setReady] = useState(false)
  const [placement, setPlacement] = useState<GuidePlacement>('top')
  const [pinned, setPinned] = useState(false)
  const [dragging, setDragging] = useState(false)

  /** write DOM; skip React re-render while scrolling */
  const displayRef = useRef({ top: 0, left: 0 })
  const targetRef = useRef({
    top: 0,
    left: 0,
    placement: 'top' as GuidePlacement,
  })
  const rafRef = useRef(0)
  const exitTimerRef = useRef(0)
  const phaseRef = useRef<FloatPhase>('closed')
  const pinnedRef = useRef(false)
  /** don't replay enter when guide content updates */
  const enteredRef = useRef(false)
  const dragSessionRef = useRef<{
    pointerId: number
    startX: number
    startY: number
    origTop: number
    origLeft: number
  } | null>(null)

  phaseRef.current = phase
  pinnedRef.current = pinned
  const isActive = phase === 'open'
  const isMounted = phase === 'open' || phase === 'closing'

  const applyDisplay = useCallback((top: number, left: number) => {
    displayRef.current = { top, left }
    const panel = panelRef.current
    if (!panel) return
    panel.style.top = `${top}px`
    panel.style.left = `${left}px`
  }, [])

  const stopSmooth = useCallback(() => {
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = 0
    }
  }, [])

  const tickSmooth = useCallback(() => {
    rafRef.current = 0
    if (phaseRef.current !== 'open') return
    if (pinnedRef.current || dragSessionRef.current) return

    const target = targetRef.current
    const cur = displayRef.current
    const reduced = prefersReducedMotion()
    const alpha = reduced ? 1 : LERP

    const nextTop = cur.top + (target.top - cur.top) * alpha
    const nextLeft = cur.left + (target.left - cur.left) * alpha
    const dx = Math.abs(target.left - nextLeft)
    const dy = Math.abs(target.top - nextTop)

    if (dx < SNAP_EPS && dy < SNAP_EPS) {
      applyDisplay(target.top, target.left)
      return
    }

    applyDisplay(nextTop, nextLeft)
    rafRef.current = requestAnimationFrame(tickSmooth)
  }, [applyDisplay])

  const startSmooth = useCallback(() => {
    if (rafRef.current) return
    if (pinnedRef.current) return
    rafRef.current = requestAnimationFrame(tickSmooth)
  }, [tickSmooth])

  const finishUnmount = useCallback(() => {
    stopSmooth()
    dragSessionRef.current = null
    setDragging(false)
    setPinned(false)
    pinnedRef.current = false
    setReady(false)
    setPhase('closed')
  }, [stopSmooth])

  /** pinned: ignore close unless force */
  const close = useCallback(
    (opts?: { force?: boolean }) => {
      const force = opts?.force === true
      if (!shouldAllowGuideClose(pinnedRef.current, force)) return
      if (phaseRef.current === 'closed' || phaseRef.current === 'closing') return
      stopSmooth()
      dragSessionRef.current = null
      setDragging(false)
      setPinned(false)
      pinnedRef.current = false
      setReady(false)

      if (prefersReducedMotion()) {
        finishUnmount()
        return
      }

      setPhase('closing')
      window.clearTimeout(exitTimerRef.current)
      exitTimerRef.current = window.setTimeout(finishUnmount, EXIT_MS)
    },
    [finishUnmount, stopSmooth],
  )

  /** pinned: no follow / no auto-close on hide */
  const measureTarget = useCallback(
    (opts?: { snap?: boolean }): boolean => {
      const trigger = triggerRef.current
      const panel = panelRef.current
      if (!trigger || !panel || phaseRef.current !== 'open') return false

      const vw = window.innerWidth
      const vh = window.innerHeight

      if (pinnedRef.current) {
        const panelW = panel.offsetWidth
        const panelH = panel.offsetHeight
        if (panelW === 0 || panelH === 0) return true
        const cur = displayRef.current
        const next = clampPanelToViewport(
          cur.top,
          cur.left,
          panelW,
          panelH,
          vw,
          vh,
        )
        if (next.top !== cur.top || next.left !== cur.left) {
          applyDisplay(next.top, next.left)
          targetRef.current = {
            ...targetRef.current,
            top: next.top,
            left: next.left,
          }
        }
        return true
      }

      const visibilityEl = resolveVisibilityTarget(trigger)
      if (!isAnchorVisible(visibilityEl) || !isAnchorVisible(trigger)) {
        close()
        return false
      }

      const rect = trigger.getBoundingClientRect()
      const panelW =
        panel.offsetWidth ||
        Math.min(PANEL_MAX_W, vw - GUIDE_VIEWPORT_PAD * 2)
      const panelH = panel.offsetHeight
      if (panelW === 0 || panelH === 0) return true

      const next = computeGuidePosition(
        domRectToGuideRect(rect),
        panelW,
        panelH,
        vw,
        vh,
      )
      targetRef.current = next
      setPlacement(next.placement)

      if (opts?.snap || prefersReducedMotion()) {
        stopSmooth()
        applyDisplay(next.top, next.left)
      } else {
        startSmooth()
      }

      return true
    },
    [applyDisplay, close, startSmooth, stopSmooth],
  )

  useEffect(() => {
    if (requireShowDetails && !help?.showDetails && !pinnedRef.current) {
      close()
    }
  }, [requireShowDetails, help?.showDetails, close])

  useEffect(
    () => () => {
      window.clearTimeout(exitTimerRef.current)
      stopSmooth()
    },
    [stopSmooth],
  )

  const openFloat = useCallback(() => {
    window.clearTimeout(exitTimerRef.current)
    setPhase('open')
    setReady(false)
  }, [])

  const toggle = useCallback(
    (e?: React.MouseEvent) => {
      e?.preventDefault()
      e?.stopPropagation()
      if (phaseRef.current === 'open') {
        if (!shouldToggleCloseGuide(true, pinnedRef.current)) return
        close({ force: true })
      } else if (phaseRef.current === 'closing') {
        window.clearTimeout(exitTimerRef.current)
        openFloat()
      } else {
        openFloat()
      }
    },
    [close, openFloat],
  )

  const togglePin = useCallback(
    (e?: React.MouseEvent) => {
      e?.preventDefault()
      e?.stopPropagation()
      if (phaseRef.current !== 'open') return

      setPinned((prev) => {
        const next = !prev
        pinnedRef.current = next
        if (next) {
          stopSmooth()
        } else {
          requestAnimationFrame(() => {
            measureTarget({ snap: true })
          })
        }
        return next
      })
    },
    [measureTarget, stopSmooth],
  )

  const onDragPointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (!pinnedRef.current || phaseRef.current !== 'open') return
      if (e.button !== 0) return
      e.preventDefault()
      e.stopPropagation()

      const panel = panelRef.current
      if (!panel) return

      stopSmooth()
      const cur = displayRef.current
      dragSessionRef.current = {
        pointerId: e.pointerId,
        startX: e.clientX,
        startY: e.clientY,
        origTop: cur.top,
        origLeft: cur.left,
      }
      setDragging(true)
      try {
        panel.setPointerCapture(e.pointerId)
      } catch {
        /* ignore */
      }
    },
    [stopSmooth],
  )

  const onDragPointerMove = useCallback(
    (e: React.PointerEvent) => {
      const session = dragSessionRef.current
      if (!session || session.pointerId !== e.pointerId) return
      const panel = panelRef.current
      if (!panel) return

      const next = applyGuideDragDelta(
        session.origTop,
        session.origLeft,
        e.clientX - session.startX,
        e.clientY - session.startY,
        panel.offsetWidth,
        panel.offsetHeight,
        window.innerWidth,
        window.innerHeight,
      )
      applyDisplay(next.top, next.left)
      targetRef.current = {
        ...targetRef.current,
        top: next.top,
        left: next.left,
      }
    },
    [applyDisplay],
  )

  const endDrag = useCallback((e?: React.PointerEvent) => {
    const session = dragSessionRef.current
    if (!session) return
    if (e && session.pointerId !== e.pointerId) return
    const panel = panelRef.current
    if (panel && e) {
      try {
        panel.releasePointerCapture(e.pointerId)
      } catch {
        /* ignore */
      }
    }
    dragSessionRef.current = null
    setDragging(false)
  }, [])

  /* snap hidden, then is-ready; don't clear ready on guide rerender */
  useLayoutEffect(() => {
    if (phase !== 'open') {
      if (phase === 'closed') stopSmooth()
      if (phase === 'closed' || phase === 'closing') {
        enteredRef.current = false
      }
      return
    }
    // pinned guide rerender: don't snap to anchor
    if (pinnedRef.current) {
      enteredRef.current = true
      setReady(true)
      return
    }
    const replayEnter = !enteredRef.current
    if (replayEnter) setReady(false)
    // content grew: lerp only, don't snap
    measureTarget({ snap: replayEnter })
    if (!replayEnter) return
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      measureTarget({ snap: true })
      raf2 = requestAnimationFrame(() => {
        if (phaseRef.current !== 'open') return
        enteredRef.current = true
        setReady(true)
      })
    })
    return () => {
      cancelAnimationFrame(raf1)
      if (raf2) cancelAnimationFrame(raf2)
    }
  }, [phase, guide, measureTarget, stopSmooth])

  /* unpinned: follow + close if hidden; pinned: clamp on resize */
  useEffect(() => {
    if (phase !== 'open') return

    const onScrollOrResize = () => {
      measureTarget({ snap: false })
    }

    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (pinnedRef.current) return
      e.stopPropagation()
      close()
    }

    const onPointerDown = (e: MouseEvent | PointerEvent) => {
      if (pinnedRef.current) return
      const node = e.target as Node | null
      if (!node) return
      if (panelRef.current?.contains(node)) return
      if (triggerRef.current?.contains(node)) return
      close()
    }

    window.addEventListener('keydown', onKey)
    const tid = window.setTimeout(() => {
      document.addEventListener('pointerdown', onPointerDown, true)
    }, 0)

    const trigger = triggerRef.current
    const visibilityEl = trigger ? resolveVisibilityTarget(trigger) : null
    let io: IntersectionObserver | null = null
    if (
      visibilityEl &&
      typeof IntersectionObserver !== 'undefined' &&
      !pinnedRef.current
    ) {
      io = new IntersectionObserver(
        (entries) => {
          if (pinnedRef.current) return
          for (const entry of entries) {
            if (entry.target !== visibilityEl) continue
            if (!entry.isIntersecting || entry.intersectionRatio <= 0.02) {
              close()
            }
          }
        },
        { threshold: [0, 0.02, 0.1, 0.5, 1] },
      )
      io.observe(visibilityEl)
    }

    return () => {
      window.removeEventListener('scroll', onScrollOrResize, true)
      window.removeEventListener('resize', onScrollOrResize)
      window.removeEventListener('keydown', onKey)
      window.clearTimeout(tid)
      document.removeEventListener('pointerdown', onPointerDown, true)
      io?.disconnect()
      stopSmooth()
    }
  }, [phase, pinned, close, measureTarget, stopSmooth])

  if (requireShowDetails && !help?.showDetails && !pinned) return null
  if (guide == null || guide === false || guide === '') return null

  const heading = format(t.config.optionGuideHeading, { title })
  const openAria = format(t.config.openOptionGuide, { title })
  const closeAria = t.common.close
  const pinAria = pinned
    ? format(t.config.unpinOptionGuideAria, { title })
    : format(t.config.pinOptionGuideAria, { title })
  const triggerLabel = isActive
    ? (closeLabel ?? t.config.hideOptionGuide)
    : (openLabel ?? t.config.optionGuide)
  const triggerAria = isActive
    ? pinned
      ? pinAria // pinned: don't imply "click to close"
      : format(t.config.hideOptionGuideAria, { title })
    : openAria

  const canPortal = typeof document !== 'undefined'

  const floating =
    isMounted && canPortal
      ? createPortal(
          <div
            ref={panelRef}
            id={panelId}
            role="dialog"
            aria-modal="false"
            aria-label={heading}
            data-pinned={pinned ? 'true' : undefined}
            data-dragging={dragging ? 'true' : undefined}
            className={[
              'setting-title-guide-float',
              `setting-title-guide-float--${placement}`,
              ready && phase === 'open' ? 'is-ready' : '',
              phase === 'closing' ? 'is-leaving' : '',
              pinned ? 'is-pinned' : '',
              dragging ? 'is-dragging' : '',
              panelClassName,
            ]
              .filter(Boolean)
              .join(' ')}
            onClick={(e) => e.stopPropagation()}
            onPointerMove={onDragPointerMove}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
            onTransitionEnd={(e) => {
              if (e.target !== panelRef.current) return
              if (e.propertyName !== 'opacity' && e.propertyName !== 'transform')
                return
              if (phaseRef.current === 'closing') {
                window.clearTimeout(exitTimerRef.current)
                finishUnmount()
              }
            }}
          >
            {pinned && (
              <div
                className="setting-title-guide-drag"
                onPointerDown={onDragPointerDown}
                role="separator"
                aria-orientation="horizontal"
                aria-label={t.config.optionGuideDragHint}
                title={t.config.optionGuideDragHint}
              >
                <LuGripVertical aria-hidden className="setting-title-guide-drag-icon" />
                <span className="setting-title-guide-drag-label">
                  {t.config.optionGuideDragHint}
                </span>
              </div>
            )}

            <div className="setting-title-guide-actions">
              <button
                type="button"
                className={[
                  'setting-title-guide-pin',
                  pinned ? 'is-pinned' : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                onClick={(e) => togglePin(e)}
                aria-label={pinAria}
                aria-pressed={pinned}
                title={pinned ? t.config.unpinOptionGuide : t.config.pinOptionGuide}
              >
                <LuPin aria-hidden />
              </button>
              <button
                type="button"
                className="setting-title-guide-close"
                onClick={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                  close({ force: true })
                }}
                aria-label={closeAria}
              >
                <FaTimes aria-hidden />
              </button>
            </div>

            <div className="setting-title-guide-body">{guide}</div>
          </div>,
          document.body,
        )
      : null

  const triggerApi: SettingTitleGuideTriggerApi = {
    open: isActive,
    closing: phase === 'closing',
    toggle,
    panelId,
    mounted: isMounted,
    ariaLabel: triggerAria,
    pinned,
  }

  const defaultTrigger = (
    <button
      ref={triggerRef as React.RefObject<HTMLButtonElement>}
      type="button"
      className={[
        'setting-title-guide-trigger',
        isActive || phase === 'closing' ? 'is-active' : '',
        pinned ? 'is-pinned' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      aria-label={triggerAria}
      aria-expanded={isActive}
      aria-controls={isMounted ? panelId : undefined}
      title={triggerAria}
      onClick={toggle}
    >
      {triggerLabel}
      {pinned ? (
        <LuPin className="setting-title-guide-trigger-pin" aria-hidden />
      ) : null}
    </button>
  )

  return (
    <span
      className={[
        'setting-title-guide',
        isActive || phase === 'closing' ? 'is-open' : '',
        pinned ? 'is-pinned' : '',
        renderTrigger ? 'has-custom-trigger' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      {renderTrigger ? (
        <span
          ref={triggerRef as React.RefObject<HTMLSpanElement>}
          className="setting-title-guide-trigger-host"
        >
          {renderTrigger(triggerApi)}
        </span>
      ) : (
        defaultTrigger
      )}
      {floating}
    </span>
  )
}

SettingTitleGuideEntry.displayName = 'SettingTitleGuideEntry'

export default SettingTitleGuideEntry
