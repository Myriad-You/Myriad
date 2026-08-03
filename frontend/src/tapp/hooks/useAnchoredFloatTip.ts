/**
 * 锚定浮层生命周期 — 安装/卸载等 tip 共用
 *
 * 防竞态要点：
 * 1. isOpen effect 只依赖 isOpen，不因 onCancel 引用变化而重开/重置
 * 2. session 代际：关闭动画回调在已 reopen 时丢弃
 * 3. onRequestClose 走 ref；父级已 isOpen=false 时清掉 pending notify，避免双重 onCancel
 * 4. 关闭中再次 isOpen=true 会清 exit timer 并直接进入 open
 * 5. 点外关闭：仅 phase=open 且 ready 后挂载；忽略 panel / anchor；busy 时可拦
 * 6. 打开同帧的 pointer 不会误关（ready 后才监听）
 */

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

  let placement: AnchoredFloatPlacement = 'top'
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
    scores.sort((a, b) => b.s - a.s)
    placement = scores[0]!.p
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
  /**
   * 用户取消 / 点外 / Esc 在退出动画结束后调用。
   * 存 ref，不进入 isOpen 同步 effect 依赖。
   */
  onRequestClose?: () => void
  /**
   * 内容尺寸变化时触发重新测量（如 appName、error、loading）。
   * 勿放不稳定引用。
   */
  contentKey?: string | number | boolean | null
  exitMs?: number
  /** 每次进入 open 时调用（重置表单等），同步执行 */
  onEnter?: () => void
  /**
   * 是否允许点外 / Esc 关闭。安装中、卸载中应传 false。
   * 每轮 render 同步进 ref。
   */
  canDismiss?: boolean
  /** 默认 true：panel 与 anchor 之外的 pointerdown 关闭 */
  closeOnOutsidePress?: boolean
  /** 默认 true：Escape 关闭 */
  closeOnEscape?: boolean
}

export interface UseAnchoredFloatTipResult {
  panelRef: RefObject<HTMLDivElement | null>
  phase: AnchoredFloatPhase
  ready: boolean
  placement: AnchoredFloatPlacement
  isMounted: boolean
  /** 当前 open 代际快照；异步开始时记下，结束后用 isCurrentSession 校验 */
  session: number
  /** 比对 ref 中的最新代际（避免 await 后闭包 session 过期） */
  isCurrentSession: (started: number) => boolean
  close: (opts?: { notifyParent?: boolean }) => void
  handlePanelTransitionEnd: (e: TransitionEvent<HTMLDivElement>) => void
  /** 拼 class：base + --placement + is-ready / is-leaving */
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
  /** 打开/重开代际：关闭回调与 setReady 必须匹配 */
  const sessionRef = useRef(0)
  /** 发起 close 时的 session；finish 时校验 */
  const closingSessionRef = useRef(0)
  const notifyParentOnCloseRef = useRef(false)
  const onRequestCloseRef = useRef(onRequestClose)
  const onEnterRef = useRef(onEnter)
  const anchorRef = useRef(anchorEl)
  const canDismissRef = useRef(canDismiss)
  const closeOnOutsidePressRef = useRef(closeOnOutsidePress)
  const closeOnEscapeRef = useRef(closeOnEscape)
  /** 父级 isOpen 的最新值，供 dismiss 与 finish 判断 */
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
      // 已 reopen 或并非本次关闭 → 丢弃
      if (sessionRef.current !== closingSession) return
      // 仅收尾仍处于 closing 的会话（reopen 会把 phase 改回 open）
      if (phaseRef.current !== 'closing') return

      stopSmooth()
      setReady(false)
      phaseRef.current = 'closed'
      setPhase('closed')

      // notify 仅在用户取消/点外/Esc 时为 true；
      // 父级 isOpen→false 的 effect 会先清掉 flag，避免双重 onCancel
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
        // 显式不 notify：覆盖先前的 cancel 意图（父级已接管）
        notifyParentOnCloseRef.current = false
      }

      // 已在 closing：只合并 notify 意图，不重启动画 / 不换 session
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

  /* 父级 isOpen → 本地 phase（仅依赖 isOpen） */
  useEffect(() => {
    if (isOpen) {
      window.clearTimeout(exitTimerRef.current)
      exitTimerRef.current = 0
      // reopen：丢弃关闭后 notify
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

    // 父级已关掉：清掉 pending notify，避免 finish 再调 onCancel
    notifyParentOnCloseRef.current = false

    if (phaseRef.current === 'open') {
      closeRef.current({ notifyParent: false })
    }
    // closing / closed：保持，由 finishUnmount 收尾
  }, [isOpen])

  useEffect(
    () => () => {
      window.clearTimeout(exitTimerRef.current)
      stopSmooth()
    },
    [stopSmooth],
  )

  /* 打开瞬间：snap → 双 rAF → is-ready */
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

  /* 滚动 / 缩放：平抑跟随 */
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

  /* 锚点 DOM 替换时重测（同一 isOpen 会话） */
  useEffect(() => {
    if (phase !== 'open') return
    measureTarget({ snap: true })
  }, [anchorEl, phase, measureTarget])

  /**
   * 点外关闭 + Escape
   * - 仅 ready 后启用，避开打开按钮同一 pointer 序列误关
   * - capture 阶段：先于内部按钮，但仍 ignore panel/anchor
   * - busy（canDismiss=false）忽略
   */
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

      // 不 preventDefault：让外层按钮等仍可响应；本 tip 只负责收起
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
