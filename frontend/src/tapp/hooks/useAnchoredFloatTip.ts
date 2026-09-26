import type { RefObject, TransitionEvent } from 'react'
import {

  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'

export type AnchoredFloatPlacement = 'top' | 'left' | 'right' | 'bottom'
export type AnchoredFloatPhase = 'closed' | 'open' | 'closing'

const VIEWPORT_PAD = 10
const GAP = 8
const LERP = 0.12
const SNAP_EPS = 0.45
const DEFAULT_EXIT_MS = 260

function clamp(n: number, min: number, max: number) {
  return Math.max(min, Math.min(n, max))
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function computePosition(
  trigger: DOMRect,
  panelW: number,
  panelH: number,
): { top: number; left: number; placement: AnchoredFloatPlacement } {
  const vw = window.innerWidth
  const vh = window.innerHeight

  const spaceAbove = trigger.top - VIEWPORT_PAD
  const spaceBelow = vh - trigger.bottom - VIEWPORT_PAD
  const spaceLeft = trigger.left - VIEWPORT_PAD
  const spaceRight = vw - trigger.right - VIEWPORT_PAD

  const needH = panelH + GAP
  const needW = panelW + GAP

  let placement: AnchoredFloatPlacement
  if (needH <= spaceAbove) {
    placement = 'top'
  } else if (needW <= spaceLeft) {
    placement = 'left'
  } else if (needW <= spaceRight) {
    placement = 'right'
  } else if (needH <= spaceBelow) {
    placement = 'bottom'
  } else {
    const scores: Array<{ p: AnchoredFloatPlacement; s: number }> = [
      { p: 'top', s: spaceAbove },
      { p: 'left', s: spaceLeft },
      { p: 'right', s: spaceRight },
      { p: 'bottom', s: spaceBelow },
    ]
    placement = scores.toSorted((a, b) => b.s - a.s)[0]!.p
  }

  let top = 0
  let left = 0

  switch (placement) {
    case 'top':
      top = trigger.top - GAP - panelH
      left = trigger.left
      break
    case 'left':
      top = trigger.top
      left = trigger.left - GAP - panelW
      break
    case 'right':
      top = trigger.top
      left = trigger.right + GAP
      break
    case 'bottom':
      top = trigger.bottom + GAP
      left = trigger.left
      break
  }

  left = clamp(left, VIEWPORT_PAD, vw - panelW - VIEWPORT_PAD)
  top = clamp(top, VIEWPORT_PAD, vh - panelH - VIEWPORT_PAD)

  return { top, left, placement }
}

function nodeContains(
  root: EventTarget | null | undefined,
  node: EventTarget | null,
): boolean {
  if (!root || !node) return false
  if (!(root instanceof Node) || !(node instanceof Node)) return false
  return root === node || root.contains(node)
}

export interface UseAnchoredFloatTipOptions {
  isOpen: boolean
  anchorEl?: HTMLElement | null
  onRequestClose?: () => void
  contentKey?: string | number | boolean | null
  exitMs?: number
  onEnter?: () => void
  canDismiss?: boolean
  closeOnOutsidePress?: boolean
  closeOnEscape?: boolean
}

export interface UseAnchoredFloatTipResult {
  panelRef: RefObject<HTMLDivElement | null>
  phase: AnchoredFloatPhase
  ready: boolean
  placement: AnchoredFloatPlacement
  isMounted: boolean
  session: number
  isCurrentSession: (started: number) => boolean
  close: (opts?: { notifyParent?: boolean }) => void
  handlePanelTransitionEnd: (e: TransitionEvent<HTMLDivElement>) => void
  className: (base: string) => string
}

export function useAnchoredFloatTip({
  isOpen,
  anchorEl = null,
  onRequestClose,
  contentKey,
  exitMs = DEFAULT_EXIT_MS,
  onEnter,
  canDismiss = true,
  closeOnOutsidePress = true,
  closeOnEscape = true,
}: UseAnchoredFloatTipOptions): UseAnchoredFloatTipResult {
  const panelRef = useRef<HTMLDivElement>(null)
  const [phase, setPhase] = useState<AnchoredFloatPhase>('closed')
  const [ready, setReady] = useState(false)
  const [placement, setPlacement] = useState<AnchoredFloatPlacement>('top')
  const [session, setSession] = useState(0)

  const displayRef = useRef({ top: 0, left: 0 })
  const targetRef = useRef({
    top: 0,
    left: 0,
    placement: 'top' as AnchoredFloatPlacement,
  })
  const rafRef = useRef(0)
  const exitTimerRef = useRef(0)
  const phaseRef = useRef<AnchoredFloatPhase>('closed')
  const sessionRef = useRef(0)
  const closingSessionRef = useRef(0)
  const notifyParentOnCloseRef = useRef(false)
  const onRequestCloseRef = useRef(onRequestClose)
  const onEnterRef = useRef(onEnter)
  const anchorRef = useRef(anchorEl)
  const canDismissRef = useRef(canDismiss)
  const closeOnOutsidePressRef = useRef(closeOnOutsidePress)
  const closeOnEscapeRef = useRef(closeOnEscape)
  const isOpenRef = useRef(isOpen)

  phaseRef.current = phase
  onRequestCloseRef.current = onRequestClose
  onEnterRef.current = onEnter
  anchorRef.current = anchorEl
  canDismissRef.current = canDismiss
  closeOnOutsidePressRef.current = closeOnOutsidePress
  closeOnEscapeRef.current = closeOnEscape
  isOpenRef.current = isOpen

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

    const target = targetRef.current
    const cur = displayRef.current
    const alpha = prefersReducedMotion() ? 1 : LERP

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
    rafRef.current = requestAnimationFrame(tickSmooth)
  }, [tickSmooth])

  const finishUnmount = useCallback(
    (closingSession: number) => {
      if (sessionRef.current !== closingSession) return
      if (phaseRef.current !== 'closing') return

      stopSmooth()
      setReady(false)
      phaseRef.current = 'closed'
      setPhase('closed')

      // 父级已关掉时清掉 pending notify，避免双重 onCancel。
      const shouldNotify = notifyParentOnCloseRef.current
      notifyParentOnCloseRef.current = false
      if (shouldNotify) {
        onRequestCloseRef.current?.()
      }
    },
    [stopSmooth],
  )

  const close = useCallback(
    (opts?: { notifyParent?: boolean }) => {
      const current = phaseRef.current
      if (current === 'closed') return

      if (opts?.notifyParent === true) {
        notifyParentOnCloseRef.current = true
      } else if (opts?.notifyParent === false) {
        notifyParentOnCloseRef.current = false
      }

      if (current === 'closing') return

      const closingSession = sessionRef.current
      closingSessionRef.current = closingSession
      stopSmooth()
      setReady(false)
      phaseRef.current = 'closing'
      setPhase('closing')

      if (prefersReducedMotion()) {
        finishUnmount(closingSession)
        return
      }

      window.clearTimeout(exitTimerRef.current)
      exitTimerRef.current = window.setTimeout(() => {
        finishUnmount(closingSession)
      }, exitMs)
    },
    [exitMs, finishUnmount, stopSmooth],
  )

  const closeRef = useRef(close)
  closeRef.current = close

  const measureTarget = useCallback(
    (opts?: { snap?: boolean }): boolean => {
      const panel = panelRef.current
      if (!panel || phaseRef.current !== 'open') return false

      const panelW = panel.offsetWidth
      const panelH = panel.offsetHeight
      if (panelW < 1 || panelH < 1) return true

      const anchor = anchorRef.current
      if (!anchor) {
        const vw = window.innerWidth
        const vh = window.innerHeight
        const next = {
          top: Math.max(VIEWPORT_PAD, vh * 0.35),
          left: Math.max(VIEWPORT_PAD, (vw - panelW) / 2),
          placement: 'bottom' as AnchoredFloatPlacement,
        }
        targetRef.current = next
        setPlacement(next.placement)
        if (opts?.snap || prefersReducedMotion()) {
          stopSmooth()
          applyDisplay(next.top, next.left)
        } else {
          startSmooth()
        }
        return true
      }

      const rect = anchor.getBoundingClientRect()
      const next = computePosition(rect, panelW, panelH)
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
    [applyDisplay, startSmooth, stopSmooth],
  )

  useEffect(() => {
    if (isOpen) {
      window.clearTimeout(exitTimerRef.current)
      exitTimerRef.current = 0
      notifyParentOnCloseRef.current = false
      sessionRef.current += 1
      const nextSession = sessionRef.current
      setSession(nextSession)
      setReady(false)
      phaseRef.current = 'open'
      setPhase('open')
      onEnterRef.current?.()
      return
    }

    notifyParentOnCloseRef.current = false

    if (phaseRef.current === 'open') {
      closeRef.current({ notifyParent: false })
    }
  }, [isOpen])

  useEffect(
    () => () => {
      window.clearTimeout(exitTimerRef.current)
      stopSmooth()
    },
    [stopSmooth],
  )

  useLayoutEffect(() => {
    if (phase !== 'open') {
      if (phase === 'closed') stopSmooth()
      return
    }

    const openSession = sessionRef.current
    setReady(false)
    measureTarget({ snap: true })
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      if (sessionRef.current !== openSession || phaseRef.current !== 'open')
        return
      measureTarget({ snap: true })
      raf2 = requestAnimationFrame(() => {
        if (sessionRef.current !== openSession || phaseRef.current !== 'open')
          return
        setReady(true)
      })
    })
    return () => {
      cancelAnimationFrame(raf1)
      if (raf2) cancelAnimationFrame(raf2)
    }
  }, [phase, session, contentKey, measureTarget, stopSmooth])

  useEffect(() => {
    if (phase !== 'open') return

    const onScrollOrResize = () => {
      measureTarget({ snap: false })
    }

    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)

    return () => {
      window.removeEventListener('scroll', onScrollOrResize, true)
      window.removeEventListener('resize', onScrollOrResize)
      stopSmooth()
    }
  }, [phase, measureTarget, stopSmooth])

  useEffect(() => {
    if (phase !== 'open') return
    measureTarget({ snap: true })
  }, [anchorEl, phase, measureTarget])

  /** 仅 ready 后启用点外关闭，避开打开按钮同一 pointer。 */
  useEffect(() => {
    if (phase !== 'open' || !ready) return
    if (typeof document === 'undefined') return

    const openSession = sessionRef.current

    const dismissIfAllowed = () => {
      if (sessionRef.current !== openSession) return
      if (phaseRef.current !== 'open') return
      if (!canDismissRef.current) return
      if (!isOpenRef.current) return
      closeRef.current({ notifyParent: true })
    }

    const onPointerDown = (e: PointerEvent) => {
      if (!closeOnOutsidePressRef.current) return
      if (sessionRef.current !== openSession) return
      if (phaseRef.current !== 'open') return
      if (!canDismissRef.current) return

      const target = e.target
      if (nodeContains(panelRef.current, target)) return
      if (nodeContains(anchorRef.current, target)) return

      dismissIfAllowed()
    }

    const onKeyDown = (e: KeyboardEvent) => {
      if (!closeOnEscapeRef.current) return
      if (e.key !== 'Escape') return
      if (sessionRef.current !== openSession) return
      if (phaseRef.current !== 'open') return
      if (!canDismissRef.current) return
      e.preventDefault()
      e.stopPropagation()
      dismissIfAllowed()
    }

    document.addEventListener('pointerdown', onPointerDown, true)
    document.addEventListener('keydown', onKeyDown, true)

    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true)
      document.removeEventListener('keydown', onKeyDown, true)
    }
  }, [phase, ready, session])

  const handlePanelTransitionEnd = useCallback(
    (e: TransitionEvent<HTMLDivElement>) => {
      if (e.target !== panelRef.current) return
      if (e.propertyName !== 'opacity' && e.propertyName !== 'transform') return
      if (phaseRef.current !== 'closing') return
      window.clearTimeout(exitTimerRef.current)
      exitTimerRef.current = 0
      finishUnmount(closingSessionRef.current)
    },
    [finishUnmount],
  )

  const className = useCallback(
    (base: string) =>
      [
        base,
        `${base}--${placement}`,
        ready && phase === 'open' ? 'is-ready' : '',
        phase === 'closing' ? 'is-leaving' : '',
      ]
        .filter(Boolean)
        .join(' '),
    [placement, ready, phase],
  )

  const isCurrentSession = useCallback(
    (started: number) =>
      sessionRef.current === started && phaseRef.current === 'open',
    [],
  )

  return {
    panelRef,
    phase,
    ready,
    placement,
    isMounted,
    session,
    isCurrentSession,
    close,
    handlePanelTransitionEnd,
    className,
  }
}
