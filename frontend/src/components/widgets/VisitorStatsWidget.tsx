/**
 * 访客统计小组件（访客视角）
 *
 * 主角是「你是今天第 N 位访客」——那个序号是访客唯一在这张卡里能找到自己的
 * 地方，所以它占据视觉重心，站点总量退成陪衬。
 *
 * 数据走公开端点 `/api/analytics/visitor`：站点总量 + 7 日趋势 + 调用者自己的
 * 到达序号。页面 / 来源 / 国家 / 停留等细分仍然只在设置页的 admin summary 里。
 *
 * 序号缺席有两种情况，文案要分开讲清楚，不能都显示成空白：
 * - `counted === false`：管理员会话无到达序号（小组件不展示说明文案）
 * - `counted === true` 但序号还是 null：本次访问的 beacon 还没落库
 *   （批处理最长 4s + sendBeacon 只保证入队），等 flush 事件再取一次
 */

import type { WidgetComponentProps } from '../widgetGridTypes'

import { LuEye, LuUsers } from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { fetchJson } from '../../utils/apiHelper'
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
/** sendBeacon 只确认入队，给服务端一点时间落库再回查序号 */
const FLUSH_SETTLE_MS = 900

/** 柱/线的顶部留白（%），峰值不贴顶 */
const TOP_HEADROOM = 10
const PLOT_SPAN = 100 - TOP_HEADROOM
/** 折线只展示最近 N 天 */
const TREND_DAYS = 5

interface DailyPoint {
  day: string
  views: number
  unique_visitors: number
}

interface VisitorCard {
  success?: boolean
  /** 站长关掉了访客统计总开关 */
  enabled?: boolean
  days?: number
  /** 本次调用者今天的到达序号；null = 未知 */
  your_ordinal_today?: number | null
  /** false = 该会话不计入统计（管理员） */
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
    // 内存缓存仍然有效
  }
}

async function requestCard(force = false): Promise<VisitorCard> {
  if (!force) {
    const cached = loadCachedCard()
    if (cached) return cached
  }
  if (globalFetchPromise) return globalFetchPromise

  globalFetchPromise = (async () => {
    // vid 只读不建：没被计入统计的人（比如已 opt-out）本来也没有序号可查
    const vid = peekVisitorId()
    const url = `${API_URL}/api/analytics/visitor${vid ? `?vid=${vid}` : ''}`
    const res = await fetchJson<VisitorCard>(
      url,
      { signal: AbortSignal.timeout(10000) },
      'Unable to load visitor stats',
    )
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

/** 英文要序数后缀（37th）；中日直接用数字 */
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

/**
 * 迷你趋势图：柱（浏览量）用 CSS 等宽列排布，折线（独立访客）用一层
 * `preserveAspectRatio="none"` 的 SVG 覆盖。
 *
 * 列不用 gap 而是靠内层柱体的水平内边距留缝——列宽因此严格等分，
 * 折线的 x = (i + 0.5) / n 才和柱心真正对齐（有 gap 就会系统性偏移）。
 * 非等比拉伸下用 `vector-effect: non-scaling-stroke` 保住发丝线宽。
 */
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
            // 由远到近略提亮，今日柱最实
            const opacity =
              n <= 1 ? 0.88 : 0.22 + ((i + 1) / n) * (isLast ? 0.66 : 0.42)
            return (
              <div
                key={p.day}
                className={`flex h-full min-w-0 flex-1 items-end ${padX}`}
              >
                {/*
                  柱高是静态样式，不走 motion：motionShim 在 framer-motion
                  到货前（或 chunk 加载失败后永久地）会把 `initial` 直接写成
                  内联样式，`initial={{ height: 0 }}` 就等于把整张图压平。
                  数据的几何形状不能依赖动画跑完。
                */}
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

        {/* 独立访客折线：中性色，不与柱的主题色抢注意力 */}
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

        {/* 末端点用 CSS 定位：圆点在非等比 viewBox 里会被拉成椭圆 */}
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

/** 图标 + 标签 + 值，横排轻量统计 */
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
    const { t, locale } = useI18n()
    const v = t.visitorStats
    const compact = config.size === '2x2'

    const [data, setData] = useState<VisitorCard | null>(null)
    const [loading, setLoading] = useState(true)
    const [failed, setFailed] = useState(false)
    const settleTimer = useRef<number | null>(null)

    const numberLocale =
      locale === 'zh-CN' ? 'zh-CN' : locale === 'ja-JP' ? 'ja-JP' : 'en-US'
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

      // 本次访问刚被计入 → 再取一次，把序号补上
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

    /** 折线只展示最近 TREND_DAYS 天 */
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
    /** BE disabled payload is only `{success,enabled:false}` — no today/all_time. */
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

      // Build hero/stats only after enabled checks — disabled card has no today/all_time.
      const heroBlock = ordinal != null ? (
        // 访客视角的重心：先说「你」，数字最大，单位收尾
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
            {/*
              同理不做入场动画：`initial={{ opacity: 0 }}` 会在 motion 缺席时
              把这张卡最重要的那个数字永久藏起来。卡片级入场由 WidgetGrid 负责。
            */}
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
        // 没有序号时退成今日访客数，并说明原因
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
          {/* No ordinal yet (beacon pending). Staff also have no ordinal by design —
              do not surface “admin not counted” on the public widget. */}
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

      // 主行：序号在左 · 趋势在右（宽收窄、左右拉开；高度约主行 3/4）
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
            ariaLabel={v.chartAria.replace('{n}', nDays)}
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
