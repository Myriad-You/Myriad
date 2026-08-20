/**
 * 「显示说明」开启时，标题旁「选项指南」入口。
 * 点击后以浮窗（Portal + fixed）展示大号介绍。
 * 定位：优先触发器上方；上方不够 → 左边。
 * 滚动：位置向目标平滑跟（平抑）；选项滚出视口则自动关闭（带退出动效）。
 *
 * 固定（pin）：
 * - 仅关闭按钮可关（点外 / Esc / 滚出视口 / 点触发器 均不关）
 * - 停止跟随锚点；可拖动手柄自由移动
 * - 取消固定后恢复跟随与自动关闭行为
 */

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
  /** 浮窗是否处于打开态（含退出动画中可作 active 样式） */
  open: boolean
  closing: boolean
  /** 切换开合；自定义触发器可无事件调用。固定时点触发器不会关闭 */
  toggle: (e?: React.MouseEvent) => void
  panelId: string
  mounted: boolean
  ariaLabel: string
  /** 当前是否固定 */
  pinned: boolean
}

export interface SettingTitleGuideEntryProps {
  /** 关联选项名（无障碍 / 浮窗标题） */
  title: string
  /**
   * 指南正文（通常为 SettingGuideBody）。
   * 未提供时不渲染。
   */
  guide?: ReactNode
  className?: string
  /**
   * 默认 true：仅「显示说明」开启时显示入口。
   * 设 false 用于页头常驻入口（如 AI「添加服务商」），仍走同一套浮窗。
   */
  requireShowDetails?: boolean
  /** 覆盖触发器文案（收起态）；默认「选项指南」 */
  openLabel?: string
  /** 覆盖触发器文案（展开态）；默认「收起指南」 */
  closeLabel?: string
  /** 附加到浮窗根节点（宽内容目录等） */
  panelClassName?: string
  /**
   * 自定义触发器（如与「显示说明」同款 CheckboxCard）。
   * 提供时不再渲染默认文字 chip；定位锚点为外包一层 host。
   */
  renderTrigger?: (api: SettingTitleGuideTriggerApi) => ReactNode
}

/** 浮窗目标宽度；窄屏自动收缩 */
const PANEL_MAX_W = 36 * 16 // 36rem
/** 每帧向目标靠近的比例（越小越慢、越平抑） */
const LERP = 0.12
/** 与目标距离小于此视为贴合，停 rAF */
const SNAP_EPS = 0.45
/** 退出动效时长（与 CSS --guide-float-exit-ms 对齐） */
const EXIT_MS = 260

/** 浮窗生命周期：挂载后 ready 才进入可见；closing 播退出动画后卸载 */
type FloatPhase = 'closed' | 'open' | 'closing'

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

/** 优先用最近的选项/分组锚点判断「选项是否看得见」 */
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
  const { t } = useI18n()
  const help = useSettingsHelp()
  const panelId = useId()
  /** 默认按钮或自定义触发器外包 host */
  const triggerRef = useRef<HTMLElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  const [phase, setPhase] = useState<FloatPhase>('closed')
  const [ready, setReady] = useState(false)
  const [placement, setPlacement] = useState<GuidePlacement>('top')
  const [pinned, setPinned] = useState(false)
  const [dragging, setDragging] = useState(false)

  /** 平滑跟随后的实际坐标（直接写 DOM，避免滚动时 React 重渲染） */
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
    // 固定 / 拖动中不跟随锚点
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

  /**
   * 关闭浮窗。
   * force=true：关闭按钮 / 内部强制；固定时外部关闭路径不得调用 force。
   * 固定时非 force 的 close 会被忽略。
   */
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

  /**
   * 测量目标位；snap=true 时立刻贴合（打开瞬间）。
   * 固定时不跟随、不因不可见而关闭。
   * 返回 false 表示选项已不可见（并已关闭）。
   */
  const measureTarget = useCallback(
    (opts?: { snap?: boolean }): boolean => {
      const trigger = triggerRef.current
      const panel = panelRef.current
      if (!trigger || !panel || phaseRef.current !== 'open') return false

      const vw = window.innerWidth
      const vh = window.innerHeight

      // 固定：只做视口夹紧，不跟锚点、不自动关
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

  /* 关闭「显示说明」时：未固定才收起（固定后仅关闭钮可关） */
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
        // 固定时点入口不关闭，只能点关闭钮
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
          // 固定：停在当前位置，停止跟随
          stopSmooth()
        } else {
          // 取消固定：重新贴回触发器
          requestAnimationFrame(() => {
            measureTarget({ snap: true })
          })
        }
        return next
      })
    },
    [measureTarget, stopSmooth],
  )

  /* —— 固定后拖动 —— */
  const onDragPointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (!pinnedRef.current || phaseRef.current !== 'open') return
      // 仅主指针；忽略按钮/链接上的按下（手柄本身无按钮）
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

  /* 打开瞬间：先贴合（隐藏态），再加 is-ready 触发进入动效 */
  useLayoutEffect(() => {
    if (phase !== 'open') {
      if (phase === 'closed') stopSmooth()
      return
    }
    // 固定中重渲染 guide 内容时不要 snap 回锚点
    if (pinnedRef.current) {
      setReady(true)
      return
    }
    setReady(false)
    measureTarget({ snap: true })
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      measureTarget({ snap: true })
      raf2 = requestAnimationFrame(() => {
        if (phaseRef.current === 'open') setReady(true)
      })
    })
    return () => {
      cancelAnimationFrame(raf1)
      if (raf2) cancelAnimationFrame(raf2)
    }
  }, [phase, guide, measureTarget, stopSmooth])

  /* 滚动 / 缩放：未固定时跟随+不可见则关；固定时仅 resize 夹紧 */
  useEffect(() => {
    if (phase !== 'open') return

    const onScrollOrResize = () => {
      measureTarget({ snap: false })
    }

    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // 固定时 Esc 也不关，仅关闭按钮
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

  // 固定态仍显示入口（即使关掉了「显示说明」）；未固定且依赖说明模式时隐藏
  if (requireShowDetails && !help?.showDetails && !pinned) return null
  if (guide == null || guide === false || guide === '') return null

  const heading = t.config.optionGuideHeading.replace('{title}', title)
  const openAria = t.config.openOptionGuide.replace('{title}', title)
  const closeAria = t.common.close
  const pinAria = pinned
    ? t.config.unpinOptionGuideAria.replace('{title}', title)
    : t.config.pinOptionGuideAria.replace('{title}', title)
  const triggerLabel = isActive
    ? (closeLabel ?? t.config.hideOptionGuide)
    : (openLabel ?? t.config.optionGuide)
  const triggerAria = isActive
    ? pinned
      ? pinAria // 固定时触发器文案提示已固定，不暗示「点此关闭」
      : t.config.hideOptionGuideAria.replace('{title}', title)
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

            {/* 固定 + 关闭：右下角操作组 */}
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
