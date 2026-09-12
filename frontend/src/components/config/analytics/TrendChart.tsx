import { LuBarChart3, LuList } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { SegmentedControl } from '../../settings'
import { formatCount, formatDuration, niceAxis, shortDay } from './format'

export interface TrendPoint {
  day: string
  views: number
  visitors: number
  engagementMs?: number
}

export interface TrendChartSeriesLabels {
  primary: string
  secondary: string
  chartAria: string
  tableDay?: string
  showEngagement?: boolean
  engagement?: string
}

interface TrendChartProps {
  points: TrendPoint[]
  refreshing?: boolean
  numberLocale: string
  seriesLabels?: TrendChartSeriesLabels
  independentScales?: boolean
}

const PLOT_H = 132
const TOP_PAD = 10
const AXIS_H = 18
const GUTTER_L = 38
const GUTTER_R = 34
const GUTTER_R_DUAL = 44
const BAR_MAX_W = 22
const BAR_GAP = 3
const DOT_R = 3
const END_LABEL_GAP = 14
const LABEL_MIN_W = 36
const TIP_OFFSET = 12

const SVG_H = TOP_PAD + PLOT_H + AXIS_H

/** two decimal places */
const px = (n: number): number => Math.round(n * 100) / 100

function columnPath(x: number, w: number, y: number, baseY: number): string {
  const h = baseY - y
  if (h <= 0.5) return ''
  const r = Math.min(4, w / 2, h)
  const inner = w - r * 2
  return [
    `M${px(x)} ${px(baseY)}`,
    `V${px(y + r)}`,
    `a${px(r)} ${px(r)} 0 0 1 ${px(r)} ${px(-r)}`,
    `h${px(inner)}`,
    `a${px(r)} ${px(r)} 0 0 1 ${px(r)} ${px(r)}`,
    `V${px(baseY)}`,
    'Z',
  ].join('')
}

/** callback ref; chart↔table remounts drop a one-shot observer */
function useMeasuredWidth<T extends HTMLElement>() {
  const ref = useRef<T | null>(null)
  const [node, setNode] = useState<T | null>(null)
  const [width, setWidth] = useState(0)

  const attach = useCallback((el: T | null) => {
    ref.current = el
    setNode(el)
  }, [])

  useEffect(() => {
    if (!node) return
    setWidth(node.clientWidth)
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver((entries) => {
      const next = Math.round(entries[0]?.contentRect.width ?? 0)
      setWidth((prev) => (prev === next ? prev : next))
    })
    ro.observe(node)
    return () => ro.disconnect()
  }, [node])

  return [attach, width, ref] as const
}

type ViewMode = 'chart' | 'table'

export const TrendChart: React.FC<TrendChartProps> = ({
  points,
  refreshing = false,
  numberLocale,
  seriesLabels,
  independentScales = false,
}) => {
  const { t } = useI18n()
  const a = t.config.analytics
  const primaryLabel = seriesLabels?.primary ?? a.legendViews
  const secondaryLabel = seriesLabels?.secondary ?? a.legendVisitors
  const chartAria = seriesLabels?.chartAria ?? a.dailyChartAria
  const tableDayLabel = seriesLabels?.tableDay ?? a.tableDay
  const showEngagement = seriesLabels?.showEngagement !== false
  const engagementLabel = seriesLabels?.engagement ?? a.tableEngagement
  const [attachPlot, width, wrapRef] = useMeasuredWidth<HTMLDivElement>()
  const [view, setView] = useState<ViewMode>('chart')
  const [active, setActive] = useState<number | null>(null)

  const count = formatCount
  const barAxis = useMemo(
    () =>
      niceAxis(
        Math.max(
          0,
          ...points.map((p) =>
            independentScales ? p.views : Math.max(p.views, p.visitors),
          ),
        ),
      ),
    [points, independentScales],
  )
  const lineAxis = useMemo(
    () =>
      independentScales
        ? niceAxis(Math.max(0, ...points.map((p) => p.visitors)))
        : barAxis,
    [points, independentScales, barAxis],
  )

  const gutterR = independentScales ? GUTTER_R_DUAL : GUTTER_R
  const plotW = Math.max(0, width - GUTTER_L - gutterR)
  const slot = points.length > 0 ? plotW / points.length : 0
  const barW = Math.max(3, Math.min(BAR_MAX_W, slot - BAR_GAP))
  const baseY = TOP_PAD + PLOT_H
  const yOfBar = useCallback(
    (v: number) => baseY - (Math.max(0, v) / barAxis.max) * PLOT_H,
    [barAxis.max, baseY],
  )
  const yOfLine = useCallback(
    (v: number) => baseY - (Math.max(0, v) / lineAxis.max) * PLOT_H,
    [lineAxis.max, baseY],
  )
  const xOf = useCallback(
    (i: number) => GUTTER_L + slot * i + slot / 2,
    [slot],
  )

  const labelEvery = Math.max(
    1,
    Math.ceil((points.length * LABEL_MIN_W) / Math.max(1, plotW)),
  )

  const linePoints = useMemo(
    () =>
      points
        .map((p, i) => `${px(xOf(i))},${px(yOfLine(p.visitors))}`)
        .join(' '),
    [points, xOf, yOfLine],
  )

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (points.length === 0) return
      const last = points.length - 1
      if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
        e.preventDefault()
        const dir = e.key === 'ArrowRight' ? 1 : -1
        setActive((prev) => {
          if (prev == null) return dir > 0 ? 0 : last
          return Math.min(last, Math.max(0, prev + dir))
        })
        return
      }
      if (e.key === 'Home') {
        e.preventDefault()
        setActive(0)
      } else if (e.key === 'End') {
        e.preventDefault()
        setActive(last)
      } else if (e.key === 'Escape') {
        setActive(null)
      }
    },
    [points.length],
  )

  const indexFromClientX = useCallback(
    (clientX: number): number | null => {
      const el = wrapRef.current
      if (!el || points.length === 0 || width <= 0 || slot <= 0) return null
      const rect = el.getBoundingClientRect()
      const rel = clientX - rect.left - GUTTER_L
      if (rel < -slot * 0.25 || rel > plotW + slot * 0.25) return null
      return Math.min(
        points.length - 1,
        Math.max(0, Math.floor(rel / slot)),
      )
    },
    [points.length, width, slot, plotW, wrapRef],
  )

  const scrubTo = useCallback(
    (clientX: number) => {
      const i = indexFromClientX(clientX)
      if (i != null) setActive(i)
    },
    [indexFromClientX],
  )

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      try {
        e.currentTarget.setPointerCapture(e.pointerId)
      } catch {
        /* ignore */
      }
      scrubTo(e.clientX)
    },
    [scrubTo],
  )

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (
        e.pointerType === 'mouse' ||
        e.buttons > 0 ||
        e.currentTarget.hasPointerCapture(e.pointerId)
      ) {
        scrubTo(e.clientX)
      }
    },
    [scrubTo],
  )

  const activePoint = active != null ? points[active] : undefined
  const tipCenter = active != null ? xOf(active) : 0
  const tipSide: 'right' | 'left' =
    width > 0 && tipCenter > width / 2 ? 'left' : 'right'
  const tipLeft =
    tipSide === 'right' ? tipCenter + TIP_OFFSET : tipCenter - TIP_OFFSET

  const legend =
    view === 'chart' ? (
      <ul className="site-analytics-legend">
        <li>
          <span
            className="site-analytics-legend-key site-analytics-legend-key--bar"
            aria-hidden
          />
          {primaryLabel}
        </li>
        <li>
          <span
            className="site-analytics-legend-key site-analytics-legend-key--line"
            aria-hidden
          />
          {secondaryLabel}
        </li>
      </ul>
    ) : (
      <span />
    )

  return (
    <div className="site-analytics-chart-block">
      <div className="site-analytics-chart-head">
        {legend}
        <SegmentedControl<ViewMode>
          size="sm"
          value={view}
          onChange={setView}
          ariaLabel={a.viewAria}
          options={[
            { value: 'chart', label: a.viewChart, icon: <LuBarChart3 size={13} /> },
            { value: 'table', label: a.viewTable, icon: <LuList size={13} /> },
          ]}
        />
      </div>

      {view === 'chart' ? (
        <div
          ref={attachPlot}
          className={`site-analytics-plot${refreshing ? ' is-refreshing' : ''}`}
          tabIndex={0}
          role="img"
          aria-label={chartAria}
          onKeyDown={onKeyDown}
          onBlur={() => setActive(null)}
          onPointerLeave={() => setActive(null)}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={(e) => {
            try {
              e.currentTarget.releasePointerCapture(e.pointerId)
            } catch {
              /* ignore */
            }
          }}
          style={{ touchAction: 'none' }}
        >
          {width > 0 ? (
            <svg
              width={width}
              height={SVG_H}
              viewBox={`0 0 ${width} ${SVG_H}`}
              focusable="false"
              aria-hidden
            >
              {barAxis.ticks.map((tick) => {
                const y = yOfBar(tick)
                return (
                  <g key={`bar-${tick}`}>
                    <line
                      className="site-analytics-grid"
                      x1={GUTTER_L}
                      x2={width - gutterR}
                      y1={px(y)}
                      y2={px(y)}
                    />
                    <text
                      className="site-analytics-tick"
                      x={GUTTER_L - 6}
                      y={px(y)}
                      textAnchor="end"
                      dominantBaseline="middle"
                    >
                      {count(tick, numberLocale)}
                    </text>
                  </g>
                )
              })}

              {independentScales
                ? lineAxis.ticks.map((tick) => {
                    const y = yOfLine(tick)
                    return (
                      <text
                        key={`line-${tick}`}
                        className="site-analytics-tick site-analytics-tick--secondary"
                        x={width - gutterR + 6}
                        y={px(y)}
                        textAnchor="start"
                        dominantBaseline="middle"
                      >
                        {count(tick, numberLocale)}
                      </text>
                    )
                  })
                : null}

              {active != null ? (
                <line
                  className="site-analytics-cursor"
                  x1={px(xOf(active))}
                  x2={px(xOf(active))}
                  y1={TOP_PAD}
                  y2={baseY}
                />
              ) : null}

              {points.map((p, i) => {
                const d = columnPath(
                  xOf(i) - barW / 2,
                  barW,
                  yOfBar(p.views),
                  baseY,
                )
                if (!d) return null
                return (
                  <path
                    key={p.day}
                    className={`site-analytics-bar${active === i ? ' is-active' : ''}`}
                    d={d}
                  />
                )
              })}

              {points.length > 1 ? (
                <polyline
                  className="site-analytics-line"
                  points={linePoints}
                  fill="none"
                />
              ) : null}
              {!independentScales && points.length > 0 ? (
                <text
                  className="site-analytics-end-label"
                  x={px(xOf(points.length - 1) + END_LABEL_GAP)}
                  y={px(yOfLine(points.at(-1)!.visitors))}
                  dominantBaseline="middle"
                >
                  {count(points.at(-1)!.visitors, numberLocale)}
                </text>
              ) : null}

              {active != null && points[active] ? (
                <circle
                  className="site-analytics-dot"
                  cx={px(xOf(active))}
                  cy={px(yOfLine(points[active]!.visitors))}
                  r={DOT_R}
                />
              ) : null}

              <line
                className="site-analytics-baseline"
                x1={GUTTER_L}
                x2={width - gutterR}
                y1={baseY}
                y2={baseY}
              />

              {points.map((p, i) => {
                const fromEnd = points.length - 1 - i
                if (fromEnd % labelEvery !== 0) return null
                return (
                  <text
                    key={p.day}
                    className="site-analytics-tick"
                    x={px(xOf(i))}
                    y={baseY + AXIS_H - 5}
                    textAnchor="middle"
                  >
                    {shortDay(p.day)}
                  </text>
                )
              })}

              {points.map((p, i) => (
                <rect
                  key={p.day}
                  className="site-analytics-hit"
                  x={px(GUTTER_L + slot * i)}
                  y={TOP_PAD}
                  width={px(Math.max(1, slot))}
                  height={PLOT_H}
                />
              ))}
            </svg>
          ) : null}

          {activePoint ? (
            <div
              className="site-analytics-tip"
              data-side={tipSide}
              role="status"
              style={{ left: `${px(tipLeft)}px` }}
            >
              <span className="site-analytics-tip-day">{activePoint.day}</span>
              <span className="site-analytics-tip-row">
                <span
                  className="site-analytics-legend-key site-analytics-legend-key--bar"
                  aria-hidden
                />
                <strong>{count(activePoint.views, numberLocale)}</strong>
                {primaryLabel}
              </span>
              <span className="site-analytics-tip-row">
                <span
                  className="site-analytics-legend-key site-analytics-legend-key--line"
                  aria-hidden
                />
                <strong>{count(activePoint.visitors, numberLocale)}</strong>
                {secondaryLabel}
              </span>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="site-analytics-table-wrap">
          <table className="site-analytics-table">
            <caption className="site-analytics-sr">{chartAria}</caption>
            <thead>
              <tr>
                <th scope="col">{tableDayLabel}</th>
                <th scope="col">{primaryLabel}</th>
                <th scope="col">{secondaryLabel}</th>
                {showEngagement ? (
                  <th scope="col">{engagementLabel}</th>
                ) : null}
              </tr>
            </thead>
            <tbody>
              {points.map((p) => (
                <tr key={p.day}>
                  <th scope="row">{p.day}</th>
                  <td>{count(p.views, numberLocale)}</td>
                  <td>{count(p.visitors, numberLocale)}</td>
                  {showEngagement ? (
                    <td>
                      {p.engagementMs && p.engagementMs > 0
                        ? formatDuration(p.engagementMs, numberLocale)
                        : '—'}
                    </td>
                  ) : null}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

export default TrendChart
