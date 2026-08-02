/**
 * 深度定制日期范围选择：Portal 弹出层 + 双月历范围点选。
 *
 * 定位 / 动效对齐 SettingTitleGuideEntry + SettingTitleHelp：
 * - Portal + fixed，避免 overflow 裁切
 * - 先量后显（is-ready），双 rAF 再入场
 * - 滚动/缩放 lerp 平滑跟随；锚点不可见则关闭
 * - 关闭走 is-closing 退出动效再卸载
 */

import type { IsoDate } from './dateCalendarLogic'
import { LuChevronLeft, LuChevronRight } from '@lib/icons'
import React, {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import {
  addMonths,
  applyRangeClick,
  buildMonthGrid,
  inclusiveDaySpan,

  parseIso,
  rangeDayState,
  toIso,
} from './dateCalendarLogic'
import './DateRangePopover.css'

export interface DateRangePopoverLabels {
  title: string
  from: string
  to: string
  apply: string
  clear: string
  hint: string
  weekdays: string[]
  monthTitle: (y: number, m: number) => string
  daysSelected: (n: number) => string
  prevMonth: string
  nextMonth: string
  today?: string
}

export interface DateRangePopoverProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  anchorRef: React.RefObject<HTMLElement | null>
  from: IsoDate
  to: IsoDate
  onChange: (next: { from: IsoDate; to: IsoDate }) => void
  minDate?: IsoDate
  maxDate?: IsoDate
  maxSpanDays?: number
  labels: DateRangePopoverLabels
  disabled?: boolean
}

const VIEWPORT_PAD = 10
const GAP = 8
const LERP = 0.14
const SNAP_EPS = 0.45
const MIN_VISIBLE_EDGE = 10
const EXIT_MS = 220

type Placement = 'bottom' | 'top'
type FloatPhase = 'closed' | 'open' | 'closing'

function clamp(n: number, min: number, max: number) {
  return Math.max(min, Math.min(n, max))
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function isAnchorVisible(el: HTMLElement): boolean {
  const rect = el.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const visibleH = Math.min(rect.bottom, vh) - Math.max(rect.top, 0)
  const visibleW = Math.min(rect.right, vw) - Math.max(rect.left, 0)
  return visibleH >= MIN_VISIBLE_EDGE && visibleW >= MIN_VISIBLE_EDGE
}

/**
 * 优先下方（右缘对齐锚点，贴标题行控件）；
 * 下方不够 → 上方。
 */
function computePosition(
  anchor: DOMRect,
  panelW: number,
  panelH: number,
): { top: number; left: number; placement: Placement } {
  const vw = window.innerWidth
  const vh = window.innerHeight
  const spaceBelow = vh - anchor.bottom - VIEWPORT_PAD
  const spaceAbove = anchor.top - VIEWPORT_PAD
  const needH = panelH + GAP

  let placement: Placement = 'bottom'
  if (needH > spaceBelow && spaceAbove > spaceBelow) {
    placement = 'top'
  }

  let left = anchor.right - panelW
  left = clamp(left, VIEWPORT_PAD, vw - panelW - VIEWPORT_PAD)

  let top =
    placement === 'bottom'
      ? anchor.bottom + GAP
      : anchor.top - GAP - panelH
  top = clamp(top, VIEWPORT_PAD, vh - panelH - VIEWPORT_PAD)

  return { top, left, placement }
}

function localTodayIso(maxDate?: string): string {
  const d = new Date()
  const iso = toIso(d.getFullYear(), d.getMonth() + 1, d.getDate())
  if (maxDate && iso > maxDate) return maxDate
  return iso
}

function MonthGrid({
  y,
  m,
  from,
  to,
  hover,
  minDate,
  maxDate,
  todayIso,
  weekdays,
  monthTitle,
  onPick,
  onHover,
}: {
  y: number
  m: number
  from: string
  to: string
  hover: string
  minDate?: string
  maxDate?: string
  todayIso: string
  weekdays: string[]
  monthTitle: (y: number, m: number) => string
  onPick: (iso: string) => void
  onHover: (iso: string | null) => void
}) {
  const cells = useMemo(
    () => buildMonthGrid(y, m, { min: minDate, max: maxDate }),
    [y, m, minDate, maxDate],
  )

  return (
    <div className="date-range-month">
      <div className="date-range-month-title">{monthTitle(y, m)}</div>
      <div className="date-range-weekdays" aria-hidden>
        {weekdays.map((w) => (
          <span key={w} className="date-range-weekday">
            {w}
          </span>
        ))}
      </div>
      <div className="date-range-grid" role="grid">
        {cells.map((cell, i) => {
          if (cell.kind === 'empty') {
            return <span key={`e-${i}`} className="date-range-cell is-empty" />
          }
          const state = rangeDayState(cell.iso, {
            from: from || undefined,
            to: to || undefined,
            hover: hover || undefined,
            disabled: cell.disabled,
          })
          const isToday = cell.iso === todayIso
          return (
            <button
              key={cell.iso}
              type="button"
              role="gridcell"
              className={[
                'date-range-cell',
                'is-day',
                `is-${state}`,
                isToday ? 'is-today' : '',
              ]
                .filter(Boolean)
                .join(' ')}
              disabled={cell.disabled}
              aria-label={cell.iso}
              aria-current={isToday ? 'date' : undefined}
              aria-selected={
                state === 'start' ||
                state === 'end' ||
                state === 'single' ||
                state === 'in-range'
              }
              onClick={() => onPick(cell.iso)}
              onMouseEnter={() => onHover(cell.iso)}
              onMouseLeave={() => onHover(null)}
            >
              <span className="date-range-day-num">{cell.day}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

export const DateRangePopover: React.FC<DateRangePopoverProps> = ({
  open: openProp,
  onOpenChange,
  anchorRef,
  from: fromProp,
  to: toProp,
  onChange,
  minDate,
  maxDate,
  maxSpanDays = 365,
  labels,
  disabled = false,
}) => {
  const panelId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const applyRef = useRef<HTMLButtonElement>(null)

  const [phase, setPhase] = useState<FloatPhase>('closed')
  const [ready, setReady] = useState(false)
  const [placement, setPlacement] = useState<Placement>('bottom')

  const displayRef = useRef({ top: 0, left: 0 })
  const targetRef = useRef({
    top: 0,
    left: 0,
    placement: 'bottom' as Placement,
  })
  const rafRef = useRef(0)
  const exitTimerRef = useRef(0)
  const phaseRef = useRef<FloatPhase>('closed')
  phaseRef.current = phase

  const [draftFrom, setDraftFrom] = useState(fromProp)
  const [draftTo, setDraftTo] = useState(toProp)
  const [picking, setPicking] = useState<'from' | 'to' | 'done'>('from')
  const [hover, setHover] = useState('')
  const [viewY, setViewY] = useState(() => {
    const p = parseIso(fromProp) || parseIso(maxDate || '')
    return p?.y ?? new Date().getFullYear()
  })
  const [viewM, setViewM] = useState(() => {
    const p = parseIso(fromProp) || parseIso(maxDate || '')
    return p?.m ?? new Date().getMonth() + 1
  })

  const todayIso = useMemo(() => localTodayIso(maxDate), [maxDate])
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

  const finishUnmount = useCallback(() => {
    stopSmooth()
    setReady(false)
    setPhase('closed')
  }, [stopSmooth])

  const close = useCallback(() => {
    if (phaseRef.current === 'closed' || phaseRef.current === 'closing') return
    stopSmooth()
    setReady(false)
    onOpenChange(false)

    if (prefersReducedMotion()) {
      finishUnmount()
      return
    }
    setPhase('closing')
    window.clearTimeout(exitTimerRef.current)
    exitTimerRef.current = window.setTimeout(finishUnmount, EXIT_MS)
  }, [finishUnmount, stopSmooth, onOpenChange])

  const openFloat = useCallback(() => {
    window.clearTimeout(exitTimerRef.current)
    setPhase('open')
    setReady(false)
  }, [])

  /* 受控 open → 内部 phase */
  useEffect(() => {
    if (openProp) {
      if (phaseRef.current === 'closed' || phaseRef.current === 'closing') {
        openFloat()
      }
    } else if (phaseRef.current === 'open') {
      // 父级关掉：走退出动效，但不重复 onOpenChange
      stopSmooth()
      setReady(false)
      if (prefersReducedMotion()) {
        finishUnmount()
      } else {
        setPhase('closing')
        window.clearTimeout(exitTimerRef.current)
        exitTimerRef.current = window.setTimeout(finishUnmount, EXIT_MS)
      }
    }
  }, [openProp, openFloat, stopSmooth, finishUnmount])

  useEffect(
    () => () => {
      window.clearTimeout(exitTimerRef.current)
      stopSmooth()
    },
    [stopSmooth],
  )

  // Sync draft when opened
  useEffect(() => {
    if (phase !== 'open') return
    setDraftFrom(fromProp)
    setDraftTo(toProp)
    setPicking(
      fromProp && toProp ? 'done' : fromProp && !toProp ? 'to' : 'from',
    )
    setHover('')
    const p = parseIso(fromProp) || parseIso(toProp) || parseIso(todayIso)
    if (p) {
      setViewY(p.y)
      setViewM(p.m)
    }
  }, [phase, fromProp, toProp, todayIso])

  const measureTarget = useCallback(
    (opts?: { snap?: boolean }): boolean => {
      const anchor = anchorRef.current
      const panel = panelRef.current
      if (!anchor || !panel || phaseRef.current !== 'open') return false

      if (!isAnchorVisible(anchor)) {
        close()
        return false
      }

      const rect = anchor.getBoundingClientRect()
      const panelW = panel.offsetWidth
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
    [anchorRef, applyDisplay, close, startSmooth, stopSmooth],
  )

  /* 打开：snap 定位 → 双 rAF → is-ready 入场 */
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
      raf2 = requestAnimationFrame(() => {
        if (phaseRef.current === 'open') {
          setReady(true)
          panelRef.current?.focus({ preventScroll: true })
        }
      })
    })
    return () => {
      cancelAnimationFrame(raf1)
      if (raf2) cancelAnimationFrame(raf2)
    }
  }, [phase, measureTarget, stopSmooth, draftFrom, draftTo, viewY, viewM])

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
        e.preventDefault()
        close()
        return
      }
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault()
        applyRef.current?.click()
      }
    }

    const onPointerDown = (e: MouseEvent | PointerEvent) => {
      const node = e.target as Node | null
      if (!node) return
      if (panelRef.current?.contains(node)) return
      if (anchorRef.current?.contains(node)) return
      close()
    }

    document.addEventListener('keydown', onKey)
    document.addEventListener('mousedown', onPointerDown)
    return () => {
      window.removeEventListener('scroll', onScrollOrResize, true)
      window.removeEventListener('resize', onScrollOrResize)
      document.removeEventListener('keydown', onKey)
      document.removeEventListener('mousedown', onPointerDown)
    }
  }, [phase, measureTarget, close, anchorRef])

  const nextMonth = addMonths(viewY, viewM, 1)

  const spanOk = useCallback(
    (from: string, to: string) => {
      if (!from || !to) return true
      return inclusiveDaySpan(from, to) <= maxSpanDays
    },
    [maxSpanDays],
  )

  const clampComplete = useCallback(
    (from: string, to: string): { from: string; to: string } => {
      let a = from
      let b = to
      if (a > b) {
        const t = a
        a = b
        b = t
      }
      if (!spanOk(a, b)) {
        const start = parseIso(a)
        if (start) {
          const endDt = new Date(
            Date.UTC(start.y, start.m - 1, start.d + maxSpanDays - 1),
          )
          b = toIso(
            endDt.getUTCFullYear(),
            endDt.getUTCMonth() + 1,
            endDt.getUTCDate(),
          )
        }
      }
      if (maxDate && b > maxDate) b = maxDate
      if (minDate && a < minDate) a = minDate
      if (a > b) b = a
      return { from: a, to: b }
    },
    [spanOk, maxSpanDays, maxDate, minDate],
  )

  const onPick = useCallback(
    (iso: string) => {
      if (disabled) return
      if (minDate && iso < minDate) return
      if (maxDate && iso > maxDate) return

      const activePicking: 'from' | 'to' =
        picking === 'done' ? 'from' : picking

      const next = applyRangeClick(iso, {
        from: picking === 'done' ? '' : draftFrom,
        to: picking === 'done' ? '' : draftTo,
        picking: activePicking,
      })

      if (next.complete && next.from && next.to) {
        const clamped = clampComplete(next.from, next.to)
        setDraftFrom(clamped.from)
        setDraftTo(clamped.to)
        setPicking('done')
        setHover('')
        return
      }

      setDraftFrom(next.from)
      setDraftTo(next.to)
      setPicking(next.picking)
    },
    [
      disabled,
      minDate,
      maxDate,
      draftFrom,
      draftTo,
      picking,
      clampComplete,
    ],
  )

  const apply = useCallback(() => {
    if (!draftFrom) return
    const end = draftTo || draftFrom
    const clamped = clampComplete(draftFrom, end)
    onChange(clamped)
    close()
  }, [draftFrom, draftTo, onChange, close, clampComplete])

  const clear = useCallback(() => {
    setDraftFrom('')
    setDraftTo('')
    setPicking('from')
    setHover('')
  }, [])

  const goToday = useCallback(() => {
    const t = todayIso
    if (minDate && t < minDate) return
    const p = parseIso(t)
    if (p) {
      const left = addMonths(p.y, p.m, -1)
      setViewY(left.y)
      setViewM(left.m)
    }
    setDraftFrom(t)
    setDraftTo(t)
    setPicking('done')
    setHover('')
  }, [todayIso, minDate])

  const goPrev = useCallback(() => {
    const n = addMonths(viewY, viewM, -1)
    setViewY(n.y)
    setViewM(n.m)
  }, [viewY, viewM])

  const goNext = useCallback(() => {
    const n = addMonths(viewY, viewM, 1)
    setViewY(n.y)
    setViewM(n.m)
  }, [viewY, viewM])

  if (!isMounted || typeof document === 'undefined') return null

  const span =
    draftFrom && draftTo
      ? inclusiveDaySpan(draftFrom, draftTo)
      : draftFrom
        ? 1
        : 0
  const canApply = Boolean(draftFrom)
  const pickingFrom = picking === 'from'
  const pickingTo = picking === 'to'

  const panel = (
    <div
      ref={panelRef}
      id={panelId}
      className={[
        'date-range-popover',
        `date-range-popover--${placement}`,
        ready && phase === 'open' ? 'is-ready' : '',
        phase === 'closing' ? 'is-closing' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      role="dialog"
      aria-modal="true"
      aria-label={labels.title}
      tabIndex={-1}
      style={{ top: 0, left: 0 }}
    >
      <span className="date-range-popover-caret" aria-hidden />

      <div className="date-range-popover-head">
        <span className="date-range-popover-title">{labels.title}</span>
        <div className="date-range-nav">
          <button
            type="button"
            className="date-range-nav-btn"
            aria-label={labels.prevMonth}
            onClick={goPrev}
          >
            <LuChevronLeft size={14} />
          </button>
          <button
            type="button"
            className="date-range-nav-btn"
            aria-label={labels.nextMonth}
            onClick={goNext}
          >
            <LuChevronRight size={14} />
          </button>
        </div>
      </div>

      {labels.hint ? <p className="date-range-hint">{labels.hint}</p> : null}

      <div className="date-range-summary">
        <span
          className={`date-range-summary-item${pickingFrom ? ' is-active' : ''}`}
        >
          <small>{labels.from}</small>
          <strong className={draftFrom ? undefined : 'is-empty'}>
            {draftFrom || '—'}
          </strong>
        </span>
        <span className="date-range-summary-sep" aria-hidden>
          →
        </span>
        <span
          className={`date-range-summary-item${pickingTo ? ' is-active' : ''}`}
        >
          <small>{labels.to}</small>
          <strong
            className={
              draftTo || (draftFrom && !pickingTo) ? undefined : 'is-empty'
            }
          >
            {draftTo || (pickingTo && draftFrom
                ? '…'
                : draftFrom && picking === 'done'
                  ? draftFrom
                  : '—')}
          </strong>
        </span>
        {span > 0 ? (
          <span className="date-range-summary-span">
            {labels.daysSelected(span)}
          </span>
        ) : null}
      </div>

      <div className="date-range-months">
        <MonthGrid
          y={viewY}
          m={viewM}
          from={draftFrom}
          to={draftTo}
          hover={hover}
          minDate={minDate}
          maxDate={maxDate}
          todayIso={todayIso}
          weekdays={labels.weekdays}
          monthTitle={labels.monthTitle}
          onPick={onPick}
          onHover={(iso) => setHover(iso || '')}
        />
        <MonthGrid
          y={nextMonth.y}
          m={nextMonth.m}
          from={draftFrom}
          to={draftTo}
          hover={hover}
          minDate={minDate}
          maxDate={maxDate}
          todayIso={todayIso}
          weekdays={labels.weekdays}
          monthTitle={labels.monthTitle}
          onPick={onPick}
          onHover={(iso) => setHover(iso || '')}
        />
      </div>

      <div className="date-range-footer">
        <div className="date-range-footer-left">
          {labels.today ? (
            <button
              type="button"
              className="date-range-btn date-range-btn--soft"
              onClick={goToday}
              disabled={disabled}
            >
              {labels.today}
            </button>
          ) : null}
          <button
            type="button"
            className="date-range-btn date-range-btn--ghost"
            onClick={clear}
            disabled={disabled || (!draftFrom && !draftTo)}
          >
            {labels.clear}
          </button>
        </div>
        <div className="date-range-footer-right">
          <button
            ref={applyRef}
            type="button"
            className="date-range-btn date-range-btn--primary"
            onClick={apply}
            disabled={disabled || !canApply}
          >
            {labels.apply}
          </button>
        </div>
      </div>
    </div>
  )

  return createPortal(panel, document.body)
}

export default DateRangePopover
