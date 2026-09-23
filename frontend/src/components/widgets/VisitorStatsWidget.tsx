// counted===false：管理员无序号，不展示说明。counted 但序号仍 null：beacon 还没落库。

import type { WidgetComponentProps } from '../widgetGridTypes'

import { LuEye, LuUsers } from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { apiService } from '../../services/api'
import {
  ANALYTICS_PAGEVIEW_FLUSHED_EVENT,
  peekVisitorId,
} from '../../utils/siteAnalytics'
import { formatCount } from '../config/analytics/format'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'

const CACHE_KEY = 'visitor_card_cache_v1'
const CACHE_DURATION = 60 * 1000
const POLL_INTERVAL = 5 * 60 * 1000
const FLUSH_SETTLE_MS = 900

const TOP_HEADROOM = 10
const PLOT_SPAN = 100 - TOP_HEADROOM
const TREND_DAYS = 5

interface DailyPoint {
  day: string
  views: number
  unique_visitors: number
}

interface VisitorCard {
  success?: boolean
  enabled?: boolean
  days?: number
  your_ordinal_today?: number | null
  counted?: boolean
  today: { views: number; unique_visitors: number }
  all_time: { views: number; unique_visitors: number }
  daily: DailyPoint[]
}

let globalFetchPromise: Promise<VisitorCard> | null = null
let globalCacheData: VisitorCard | null = null
let globalCacheTimestamp = 0

function loadCachedCard(): VisitorCard | null {
  if (globalCacheData && Date.now() - globalCacheTimestamp < CACHE_DURATION) {
    return globalCacheData
  }
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached) return null
    const parsed = JSON.parse(cached) as {
      data?: VisitorCard
      timestamp?: number
    }
    if (
      parsed.data?.daily &&
      typeof parsed.timestamp === 'number' &&
      Date.now() - parsed.timestamp < CACHE_DURATION
    ) {
      return parsed.data
    }
  } catch {
    localStorage.removeItem(CACHE_KEY)
  }
  return null
}

function saveCachedCard(data: VisitorCard): void {
  try {
    localStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ data, timestamp: Date.now() }),
    )
  } catch {
  }
}

async function requestCard(force = false): Promise<VisitorCard> {
  if (!force) {
    const cached = loadCachedCard()
    if (cached) return cached
  }
  if (globalFetchPromise) return globalFetchPromise

  globalFetchPromise = (async () => {
    const vid = peekVisitorId()
    const res = await apiService.get<VisitorCard>('/analytics/visitor', {
      params: { vid: vid || undefined },
      timeout: 10_000,
    })
    if (!res?.success) throw new Error('visitor card unavailable')
    globalCacheData = res
    globalCacheTimestamp = Date.now()
    saveCachedCard(res)
    return res
  })().finally(() => {
    globalFetchPromise = null
  })

  return globalFetchPromise
}

function previewCard(): VisitorCard {
  const views = [104, 178, 145, 210, 164]
  const visitors = [52, 88, 71, 96, 78]
  const today = new Date()
  return {
    success: true,
    enabled: true,
    days: 5,
    your_ordinal_today: 37,
    counted: true,
    today: { views: 164, unique_visitors: 78 },
    all_time: { views: 18420, unique_visitors: 4260 },
    daily: views.map((v, i) => {
      const d = new Date(today)
      d.setDate(d.getDate() - (views.length - 1 - i))
      return {
        day: d.toISOString().slice(0, 10),
        views: v,
        unique_visitors: visitors[i],
      }
    }),
  }
}

function formatOrdinal(n: number, locale: string): string {
  if (!locale.startsWith('en')) return String(n)
  const rem100 = n % 100
  if (rem100 >= 11 && rem100 <= 13) return `${n}th`
  switch (n % 10) {
    case 1:
      return `${n}st`
    case 2:
      return `${n}nd`
    case 3:
      return `${n}rd`
    default:
      return `${n}th`
  }
}

const MiniTrend = memo(
  ({
    points,
    ariaLabel,
    tight,
  }: {
    points: DailyPoint[]
    ariaLabel: string
    tight: boolean
  }) => {
    const max = useMemo(
      () =>
        Math.max(1, ...points.map((p) => Math.max(p.views, p.unique_visitors))),
      [points],
    )

    const n = points.length
    const linePoints = useMemo(
      () =>
        points
          .map((p, i) => {
            const x = ((i + 0.5) / n) * 100
            const y = 100 - (p.unique_visitors / max) * PLOT_SPAN
            return `${Math.round(x * 100) / 100},${Math.round(y * 100) / 100}`
          })
          .join(' '),
      [points, n, max],
    )

    const last = points[n - 1]
    const lastX = ((n - 0.5) / n) * 100
    const lastY = ((last?.unique_visitors ?? 0) / max) * PLOT_SPAN
    const padX = tight ? 'px-px' : 'px-[2px]'
    const barRadius = tight ? 1.5 : 2.5
    const endDot = tight ? 4 : 5

    return (
      <div
        className="relative h-full w-full min-w-0"
        role="img"
        aria-label={ariaLabel}
      >
        <div className="flex h-full w-full items-end">
          {points.map((p, i) => {
            const isLast = i === n - 1
            const opacity =
              n <= 1 ? 0.88 : 0.22 + ((i + 1) / n) * (isLast ? 0.66 : 0.42)
            return (
              <div
                key={p.day}
                className={`flex h-full min-w-0 flex-1 items-end ${padX}`}
              >
                {/* 柱高用静态样式；motionShim 会把 initial.height:0 写成内联，整图压平。 */}
                <div
                  className="w-full"
                  style={{
                    borderRadius: `${barRadius}px ${barRadius}px 1px 1px`,
                    background: isLast
                      ? 'var(--color-primary)'
                      : 'color-mix(in srgb, var(--color-primary) 88%, transparent)',
                    opacity,
                    height: `${Math.max(3, (p.views / max) * PLOT_SPAN)}%`,
                  }}
                />
              </div>
            )
          })}
        </div>

        <svg
          className="pointer-events-none absolute inset-0 h-full w-full overflow-visible text-gray-600/65 dark:text-gray-200/55"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          aria-hidden
        >
          <polyline
            points={linePoints}
            fill="none"
            stroke="currentColor"
            strokeWidth={tight ? 1.35 : 1.6}
            strokeLinecap="round"
            strokeLinejoin="round"
            vectorEffect="non-scaling-stroke"
          />
        </svg>

        <span
          className="pointer-events-none absolute -translate-x-1/2 translate-y-1/2 rounded-full bg-gray-700 shadow-sm ring-2 ring-white/80 dark:bg-gray-100 dark:ring-black/35"
          style={{
            left: `${lastX}%`,
            bottom: `${lastY}%`,
            width: endDot,
            height: endDot,
          }}
          aria-hidden
        />
      </div>
    )
  },
)

MiniTrend.displayName = 'MiniTrend'

const StatCell = memo(
  ({
    icon,
    label,
    value,
    fontScale,
    scale,
  }: {
    icon: React.ReactNode
    label: string
    value: string
    fontScale: number
    scale: number
  }) => (
    <span
      className="flex min-w-0 flex-1 items-center"
      style={{ gap: `${5 * scale}px` }}
    >
      <span
        className="flex shrink-0 items-center justify-center text-gray-400 dark:text-gray-500"
        aria-hidden
      >
        {icon}
      </span>
      <span className="flex min-w-0 flex-col leading-tight">
        <span
          className="truncate text-gray-500 dark:text-gray-400"
          style={{ fontSize: `${8.5 * fontScale}px` }}
        >
          {label}
        </span>
        <span
          className="truncate font-semibold tracking-tight text-gray-800 tabular-nums dark:text-gray-100"
          style={{ fontSize: `${12 * fontScale}px` }}
        >
          {value}
        </span>
      </span>
    </span>
  ),
)

StatCell.displayName = 'StatCell'

export const VisitorStatsWidget = memo(
  ({ config, isPreview }: WidgetComponentProps) => {
    const { containerRef, scale, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t, locale, format } = useI18n()
    const v = t.visitorStats
    const compact = config.size === '2x2'

    const [data, setData] = useState<VisitorCard | null>(null)
    const [loading, setLoading] = useState(true)
    const [failed, setFailed] = useState(false)
    const settleTimer = useRef<number | null>(null)

    const numberLocale = locale
    const count = useCallback(
      (n: number) => formatCount(n, numberLocale),
      [numberLocale],
    )

    const refresh = useCallback(async (force = false) => {
      try {
        const res = await requestCard(force)
        setData(res)
        setFailed(false)
      } catch (error) {
        if (error instanceof Error && error.name !== 'AbortError') {
          console.error('Failed to fetch visitor card:', error)
        }
        setFailed(true)
      } finally {
        setLoading(false)
      }
    }, [])

    useEffect(() => {
      if (isPreview) {
        setData(previewCard())
        setLoading(false)
        return
      }

      const cached = loadCachedCard()
      if (cached) {
        setData(cached)
        setLoading(false)
      }
      void refresh(false)

      const handleFlushed = () => {
        if (settleTimer.current != null) return
        settleTimer.current = window.setTimeout(() => {
          settleTimer.current = null
          void refresh(true)
        }, FLUSH_SETTLE_MS)
      }
      const handleFocus = () => void refresh(false)
      const poll = window.setInterval(() => void refresh(false), POLL_INTERVAL)
      window.addEventListener(ANALYTICS_PAGEVIEW_FLUSHED_EVENT, handleFlushed)
      window.addEventListener('focus', handleFocus)
      return () => {
        window.clearInterval(poll)
        if (settleTimer.current != null) {
          window.clearTimeout(settleTimer.current)
          settleTimer.current = null
        }
        window.removeEventListener(
          ANALYTICS_PAGEVIEW_FLUSHED_EVENT,
          handleFlushed,
        )
        window.removeEventListener('focus', handleFocus)
      }
    }, [isPreview, refresh])

    const points = useMemo(() => {
      const all = (data?.daily ?? []).filter((d) => typeof d.day === 'string')
      return all.length > TREND_DAYS ? all.slice(-TREND_DAYS) : all
    }, [data])
    const hasTrend = points.some((p) => p.views > 0 || p.unique_visitors > 0)
    const nDays = String(Math.max(1, points.length || TREND_DAYS))
    const ordinal =
      typeof data?.your_ordinal_today === 'number' &&
      data.your_ordinal_today > 0
        ? data.your_ordinal_today
        : null
    const collectionOff = data?.enabled === false

    const body = (() => {
      if (loading && !data) {
        return null
      }
      if (collectionOff) {
        return (
          <div className="flex h-full flex-col items-center justify-center text-center text-gray-500 dark:text-gray-400">
            <div className="mb-1 text-lg opacity-35">◌</div>
            <span style={{ fontSize: `${9 * fontScale}px` }}>
              {v.collectionOff}
            </span>
          </div>
        )
      }
      if (!data) {
        return (
          <div className="flex h-full flex-col items-center justify-center text-center text-gray-500 dark:text-gray-400">
            <div className="mb-1 text-lg opacity-35">◌</div>
            <span style={{ fontSize: `${9 * fontScale}px` }}>
              {failed ? v.loadFailed : v.empty}
            </span>
          </div>
        )
      }

      const heroBlock = ordinal != null ? (
        <div
          className="flex min-w-0 flex-col justify-center"
          style={{ gap: `${3 * scale}px` }}
        >
          <span
            className="truncate font-medium tracking-wide text-gray-500 dark:text-gray-400"
            style={{ fontSize: `${(compact ? 9 : 10) * fontScale}px` }}
          >
            {v.ordinalLead}
          </span>
          <span
            className="flex min-w-0 items-baseline"
            style={{ gap: `${5 * scale}px` }}
          >
            <span
              className="font-black leading-none tracking-tight text-gray-900 tabular-nums dark:text-white"
              style={{
                fontSize: `${(compact ? 30 : 36) * fontScale}px`,
                letterSpacing: '-0.03em',
              }}
            >
              {formatOrdinal(ordinal, numberLocale)}
            </span>
            <span
              className="shrink-0 font-medium text-gray-500 dark:text-gray-400"
              style={{
                fontSize: `${(compact ? 10 : 11) * fontScale}px`,
                paddingBottom: `${1 * scale}px`,
              }}
            >
              {v.ordinalTrail}
            </span>
          </span>
        </div>
      ) : (
        <div
          className="flex min-w-0 flex-col justify-center"
          style={{ gap: `${2 * scale}px` }}
        >
          <span
            className="flex items-center truncate font-medium text-gray-500 dark:text-gray-400"
            style={{
              fontSize: `${(compact ? 9 : 10) * fontScale}px`,
              gap: `${4 * scale}px`,
            }}
          >
            <LuUsers size={Math.round(11 * scale)} aria-hidden />
            {v.todayVisitors}
          </span>
          <span
            className="font-black leading-none tracking-tight text-gray-900 tabular-nums dark:text-white"
            style={{
              fontSize: `${(compact ? 30 : 36) * fontScale}px`,
              letterSpacing: '-0.03em',
            }}
          >
            {count(data?.today?.unique_visitors ?? 0)}
          </span>
          {/* 管理员本来就没有序号，公开卡片不要写「管理员不计数」。 */}
          {data?.counted !== false ? (
            <span
              className="truncate text-gray-400 dark:text-gray-500"
              style={{ fontSize: `${8 * fontScale}px` }}
            >
              {v.ordinalPending}
            </span>
          ) : null}
        </div>
      )

      const iconPx = Math.max(10, Math.round(11 * scale))
      const statRows = (
        <div
          className="flex min-w-0 shrink-0 items-stretch"
          style={{ gap: `${(compact ? 8 : 12) * scale}px` }}
        >
          <StatCell
            icon={<LuEye size={iconPx} strokeWidth={2} />}
            label={v.allTimeViewsShort}
            value={count(data?.all_time?.views ?? 0)}
            fontScale={fontScale}
            scale={scale}
          />
          <StatCell
            icon={<LuUsers size={iconPx} strokeWidth={2} />}
            label={v.allTimeVisitorsShort}
            value={count(data?.all_time?.unique_visitors ?? 0)}
            fontScale={fontScale}
            scale={scale}
          />
        </div>
      )

      const chartBox = hasTrend ? (
        <div
          className="min-w-0 shrink-0 self-center"
          style={
            compact
              ? {
                  width: `${80 * scale}px`,
                  maxWidth: '44%',
                  height: '72%',
                }
              : {
                  width: `${152 * scale}px`,
                  maxWidth: '42%',
                  height: '74%',
                }
          }
        >
          <MiniTrend
            points={points}
            tight={compact}
            ariaLabel={format(v.chartAria, { n: Number(nDays) })}
          />
        </div>
      ) : !compact ? (
        <div className="flex min-w-0 shrink items-center justify-center self-center text-gray-400 dark:text-gray-500">
          <span style={{ fontSize: `${9 * fontScale}px` }}>{v.empty}</span>
        </div>
      ) : null

      return (
        <>
          <div
            className="flex min-h-0 flex-1 items-center justify-between"
            style={{ gap: `${(compact ? 16 : 32) * scale}px` }}
          >
            <div className="flex min-w-0 shrink items-center">{heroBlock}</div>
            {chartBox}
          </div>
          <div
            className="shrink-0"
            style={{ marginTop: `${(compact ? 8 : 10) * scale}px` }}
          >
            {statRows}
          </div>
        </>
      )
    })()

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={compact ? 11 : 13}
        contentClassName="relative flex min-h-0 flex-col"
        background={
          <GlowBackground
            color="var(--color-primary)"
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="md"
            opacity={0.55}
          />
        }
      >
        <div
          className="flex shrink-0 items-center"
          style={{ marginBottom: `${(compact ? 6 : 8) * scale}px` }}
        >
          <h3
            className="truncate font-semibold tracking-wide text-gray-600 dark:text-gray-300"
            style={{
              fontSize: `${(compact ? 10 : 11) * fontScale}px`,
              letterSpacing: '0.02em',
            }}
          >
            {v.widgetTitle}
          </h3>
        </div>

        <div className="relative min-h-0 flex-1">
          {body}
          <WidgetSkeletonCover
            active={Boolean(loading && !data)}
            preset="hero"
            accent="var(--color-primary)"
            label={t.common.loading}
          />
        </div>
      </WidgetShell>
    )
  },
)

VisitorStatsWidget.displayName = 'VisitorStatsWidget'
