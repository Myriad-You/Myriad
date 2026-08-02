/**
 * 「显示说明」开启时，标题旁「选项指南」入口。
 * 点击后以浮窗（Portal + fixed）展示大号介绍。
 * 定位：优先触发器上方；上方不够 → 左边。
 * 滚动：位置向目标平滑跟（平抑）；选项滚出视口则自动关闭（带退出动效）。
 */

import type { ReactNode } from 'react'
import { FaTimes } from '@lib/icons'
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
import './SettingTitleGuideEntry.css'

export interface SettingTitleGuideEntryProps {
  /** 关联选项名（无障碍 / 浮窗标题） */
  title: string
  /**
   * 指南正文（通常为 SettingGuideBody）。
   * 未提供时不渲染。
   */
  guide?: ReactNode
  className?: string
}

const VIEWPORT_PAD = 10
const GAP = 8
/** 浮窗目标宽度；窄屏自动收缩 */
const PANEL_MAX_W = 36 * 16 // 36rem
/** 每帧向目标靠近的比例（越小越慢、越平抑） */
const LERP = 0.12
/** 与目标距离小于此视为贴合，停 rAF */
const SNAP_EPS = 0.45
/** 触发器在视口内至少保留的可见边长（px），低于则关闭 */
const MIN_VISIBLE_EDGE = 10
/** 退出动效时长（与 CSS --guide-float-exit-ms 对齐） */
const EXIT_MS = 260

type Placement = 'top' | 'left' | 'right' | 'bottom'

interface Coords {
  top: number
  left: number
  placement: Placement
}

/** 浮窗生命周期：挂载后 ready 才进入可见；closing 播退出动画后卸载 */
type FloatPhase = 'closed' | 'open' | 'closing'

function clamp(n: number, min: number, max: number) {
  return Math.max(min, Math.min(n, max))
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

/**
 * 优先上方（左缘对齐触发器）；
 * 上方高度不够 → 左边（顶缘对齐触发器）；
 * 左边宽度不够 → 右边；再不行 → 下方。
 */
function computePosition(
  trigger: DOMRect,
  panelW: number,
  panelH: number,
): Coords {
  const vw = window.innerWidth
  const vh = window.innerHeight

  const spaceAbove = trigger.top - VIEWPORT_PAD
  const spaceBelow = vh - trigger.bottom - VIEWPORT_PAD
  const spaceLeft = trigger.left - VIEWPORT_PAD
  const spaceRight = vw - trigger.right - VIEWPORT_PAD

  const needH = panelH + GAP
  const needW = panelW + GAP

  let placement: Placement = 'top'
  if (needH <= spaceAbove) {
    placement = 'top'
  } else if (needW <= spaceLeft) {
    placement = 'left'
  } else if (needW <= spaceRight) {
    placement = 'right'
  } else if (needH <= spaceBelow) {
    placement = 'bottom'
  } else {
    const scores: Array<{ p: Placement; s: number }> = [
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

/** 触发器（或所属选项锚点）是否仍在视口内可见 */
function isAnchorVisible(el: HTMLElement): boolean {
  const rect = el.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const visibleH = Math.min(rect.bottom, vh) - Math.max(rect.top, 0)
  const visibleW = Math.min(rect.right, vw) - Math.max(rect.left, 0)
  return visibleH >= MIN_VISIBLE_EDGE && visibleW >= MIN_VISIBLE_EDGE
}

/** 优先用最近的选项/分组锚点判断「选项是否看得见」 */
function resolveVisibilityTarget(trigger: HTMLElement): HTMLElement {
  const anchor = trigger.closest(
    '.has-guide-anchor, .setting-item, .setting-group, .section-header-text',
  )
  return (anchor as HTMLElement | null) ?? trigger
}

export const SettingTitleGuideEntry: React.FC<SettingTitleGuideEntryProps> = ({
  title,
  guide,
  className = '',
}) => {
  const { t } = useI18n()
  const help = useSettingsHelp()
  const panelId = useId()
  const triggerRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  const [phase, setPhase] = useState<FloatPhase>('closed')
  const [ready, setReady] = useState(false)
  const [placement, setPlacement] = useState<Placement>('top')

  /** 平滑跟随后的实际坐标（直接写 DOM，避免滚动时 React 重渲染） */
  const displayRef = useRef({ top: 0, left: 0 })
  const targetRef = useRef({ top: 0, left: 0, placement: 'top' as Placement })
  const rafRef = useRef(0)
  const exitTimerRef = useRef(0)
  const phaseRef = useRef<FloatPhase>('closed')

  phaseRef.current = phase
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
    rafRef.current = requestAnimationFrame(tickSmooth)
  }, [tickSmooth])

  const finishUnmount = useCallback(() => {
    stopSmooth()
    setReady(false)
    setPhase('closed')
  }, [stopSmooth])

  /** 带退出动效的关闭 */
  const close = useCallback(() => {
    if (phaseRef.current === 'closed' || phaseRef.current === 'closing') return
    stopSmooth()
    setReady(false)

    if (prefersReducedMotion()) {
      finishUnmount()
      return
    }

    setPhase('closing')
    window.clearTimeout(exitTimerRef.current)
    exitTimerRef.current = window.setTimeout(finishUnmount, EXIT_MS)
  }, [finishUnmount, stopSmooth])

  /**
   * 测量目标位；snap=true 时立刻贴合（打开瞬间）。
   * 返回 false 表示选项已不可见（并已关闭）。
   */
  const measureTarget = useCallback(
    (opts?: { snap?: boolean }): boolean => {
      const trigger = triggerRef.current
      const panel = panelRef.current
      if (!trigger || !panel || phaseRef.current !== 'open') return false

      const visibilityEl = resolveVisibilityTarget(trigger)
      if (!isAnchorVisible(visibilityEl) || !isAnchorVisible(trigger)) {
        close()
        return false
      }

      const rect = trigger.getBoundingClientRect()
      const panelW =
        panel.offsetWidth ||
        Math.min(PANEL_MAX_W, window.innerWidth - VIEWPORT_PAD * 2)
      const panelH = panel.offsetHeight
      if (panelW === 0 || panelH === 0) return true

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
    [applyDisplay, close, startSmooth, stopSmooth],
  )

  /* 关闭「显示说明」时带退出动效收起 */
  useEffect(() => {
    if (!help?.showDetails) {
      close()
    }
  }, [help?.showDetails, close])

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
    (e: React.MouseEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (phaseRef.current === 'open') {
        close()
      } else if (phaseRef.current === 'closing') {
        // 退出中再次点击：打断退出，重新打开
        window.clearTimeout(exitTimerRef.current)
        openFloat()
      } else {
        openFloat()
      }
    },
    [close, openFloat],
  )

  /* 打开瞬间：先贴合（隐藏态），再加 is-ready 触发进入动效 */
  useLayoutEffect(() => {
    if (phase !== 'open') {
      if (phase === 'closed') stopSmooth()
      return
    }
    setReady(false)
    measureTarget({ snap: true })
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      measureTarget({ snap: true })
      // 双 rAF：保证初始 opacity/transform 帧已绘制，再进入
      raf2 = requestAnimationFrame(() => {
        if (phaseRef.current === 'open') setReady(true)
      })
    })
    return () => {
      cancelAnimationFrame(raf1)
      if (raf2) cancelAnimationFrame(raf2)
    }
  }, [phase, guide, measureTarget, stopSmooth])

  /* 滚动 / 缩放：平抑跟随 + 选项不可见则关 */
  useEffect(() => {
    if (phase !== 'open') return

    const onScrollOrResize = () => {
      measureTarget({ snap: false })
    }

    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)

    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        close()
      }
    }

    const onPointerDown = (e: MouseEvent | PointerEvent) => {
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
    if (visibilityEl && typeof IntersectionObserver !== 'undefined') {
      io = new IntersectionObserver(
        (entries) => {
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
  }, [phase, close, measureTarget, stopSmooth])

  if (!help?.showDetails) return null
  if (guide == null || guide === false || guide === '') return null

  const heading = t.config.optionGuideHeading.replace('{title}', title)
  const openAria = t.config.openOptionGuide.replace('{title}', title)
  const closeAria = t.common.close
  const triggerLabel = isActive ? t.config.hideOptionGuide : t.config.optionGuide
  const triggerAria = isActive
    ? t.config.hideOptionGuideAria.replace('{title}', title)
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
            className={[
              'setting-title-guide-float',
              `setting-title-guide-float--${placement}`,
              ready && phase === 'open' ? 'is-ready' : '',
              phase === 'closing' ? 'is-leaving' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            onClick={(e) => e.stopPropagation()}
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
            {/* 浮动关闭：壁纸主色圆钮 */}
            <button
              type="button"
              className="setting-title-guide-close"
              onClick={(e) => {
                e.preventDefault()
                e.stopPropagation()
                close()
              }}
              aria-label={closeAria}
            >
              <FaTimes aria-hidden />
            </button>
            <div className="setting-title-guide-body">{guide}</div>
          </div>,
          document.body,
        )
      : null

  return (
    <span
      className={[
        'setting-title-guide',
        isActive || phase === 'closing' ? 'is-open' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <button
        ref={triggerRef}
        type="button"
        className={[
          'setting-title-guide-trigger',
          isActive || phase === 'closing' ? 'is-active' : '',
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
      </button>
      {floating}
    </span>
  )
}

SettingTitleGuideEntry.displayName = 'SettingTitleGuideEntry'

export default SettingTitleGuideEntry
