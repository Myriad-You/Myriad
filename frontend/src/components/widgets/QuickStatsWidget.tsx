/**
 * 内容数据概览卡片 - 4x2
 * 显示资料库统计数据
 */

import type { WidgetComponentProps } from '../WidgetGrid'
import { motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { getLibraryDataDeduped } from '../../utils/requestDedup'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'

// 缓存配置
const CACHE_KEY = 'library_stats_cache'
const CACHE_DURATION = 5 * 60 * 1000 // 5分钟

interface LibraryStats {
  total: number
  game: number
  video: number
  music: number
  anime: number
  tv_series: number
  book: number
}

// 统计卡片组件 - 避免重复渲染
const StatCard = memo(
  ({
    cat,
    value,
    loading,
    scale = 1,
    fontScale = 1,
  }: {
    cat: any
    value: number
    loading: boolean
    index: number
    scale?: number
    fontScale?: number
  }) => {
    const renderIcon = useCallback(
      (type: string) => {
        const style = { width: `${20 * scale}px`, height: `${20 * scale}px` }
        switch (type) {
          case 'game':
            return (
              <svg
                className="w-5 h-5"
                style={style}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                strokeWidth={2}
              >
                <rect x="2" y="6" width="20" height="12" rx="3" />
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M6 12h4m-2-2v4"
                />
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={3}
                  d="M15 11h.01M17 13h.01"
                />
              </svg>
            )
          case 'video':
            return (
              <svg
                className="w-5 h-5"
                style={style}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                strokeWidth={2}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z"
                />
              </svg>
            )
          case 'music':
            return (
              <svg
                className="w-5 h-5"
                style={style}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                strokeWidth={2}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M9 19V6l12-3v13M9 19c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zm12-3c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zM9 10l12-3"
                />
              </svg>
            )
          case 'anime':
            return (
              <svg
                className="w-5 h-5"
                style={style}
                fill="currentColor"
                viewBox="0 0 24 24"
              >
                <text
                  x="50%"
                  y="50%"
                  textAnchor="middle"
                  dominantBaseline="central"
                  fontSize="18"
                  fontWeight="bold"
                >
                  あ
                </text>
              </svg>
            )
          case 'tv_series':
            return (
              <svg
                className="w-5 h-5"
                style={style}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                strokeWidth={2}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M6 20.25h12m-7.5-3v3m3-3v3m-10.125-3h17.25c.621 0 1.125-.504 1.125-1.125V4.875c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v11.25c0 .621.504 1.125 1.125 1.125z"
                />
              </svg>
            )
          case 'book':
            return (
              <svg
                className="w-5 h-5"
                style={style}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                strokeWidth={2}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M4 19.5A2.5 2.5 0 016.5 17H20M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"
                />
              </svg>
            )
          default:
            return null
        }
      },
      [scale],
    )

    return (
      <div
        className="flex flex-col items-center justify-center glass-surface glass-60 rounded-lg relative overflow-hidden p-1.5"
        style={{ padding: `${6 * scale}px` }}
      >
        <div
          className="absolute top-0 right-0 rounded-full blur-xl opacity-20 w-6 h-6"
          style={{
            background: cat.color,
            width: `${24 * scale}px`,
            height: `${24 * scale}px`,
          }}
        />
        <div
          className="relative z-10 flex flex-col items-center gap-0.5"
          style={{ gap: `${2 * scale}px` }}
        >
          <div className="text-gray-700 dark:text-white/60">
            {renderIcon(cat.key)}
          </div>
          <span
            className="text-base font-black text-gray-800 dark:text-gray-200 leading-none"
            style={{ fontSize: `${16 * fontScale}px` }}
          >
            {loading ? '-' : value}
          </span>
          <span
            className="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold"
            style={{ fontSize: `${7 * fontScale}px` }}
          >
            {cat.label}
          </span>
        </div>
      </div>
    )
  },
)

StatCard.displayName = 'StatCard'

export const QuickStatsWidget = memo(
  ({ config, isPreview }: WidgetComponentProps) => {
    const { containerRef, scale, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t } = useI18n()
    const [stats, setStats] = useState<LibraryStats>({
      total: 0,
      game: 0,
      video: 0,
      music: 0,
      anime: 0,
      tv_series: 0,
      book: 0,
    })
    const [loading, setLoading] = useState(true)

    // 从缓存加载
    const loadFromCache = useCallback(() => {
      try {
        const cached = localStorage.getItem(CACHE_KEY)
        if (cached) {
          const { data, timestamp } = JSON.parse(cached)
          if (Date.now() - timestamp < CACHE_DURATION) {
            setStats(data)
            return true
          }
        }
      } catch (err) {
        console.error(`${t.quickStats.loadCacheFailed}:`, err)
      }
      return false
    }, [t])

    // 保存到缓存
    const saveToCache = useCallback(
      (data: LibraryStats) => {
        try {
          localStorage.setItem(
            CACHE_KEY,
            JSON.stringify({
              data,
              timestamp: Date.now(),
            }),
          )
        } catch (err) {
          console.error(`${t.quickStats.saveCacheFailed}:`, err)
        }
      },
      [t],
    )

    const fetchLibraryStats = useCallback(async () => {
      try {
        // 使用去重版本，避免多组件同时请求
        const data = await getLibraryDataDeduped()

        if (data.success && Array.isArray(data.items)) {
          // 使用 reduce 一次性统计，性能更好
          const counts = data.items.reduce(
            (acc: LibraryStats, item: any) => {
              acc.total++
              const type = item.item_type
              if (type in acc) {
                ;(acc as any)[type]++
              }
              return acc
            },
            {
              total: 0,
              game: 0,
              video: 0,
              music: 0,
              anime: 0,
              tv_series: 0,
              book: 0,
            },
          )

          setStats(counts)
          saveToCache(counts)
        }
      } catch (err) {
        if (err instanceof Error && err.name !== 'AbortError') {
          console.error(`${t.quickStats.fetchStatsFailed}:`, err)
        }
      } finally {
        setLoading(false)
      }
    }, [saveToCache, t])

    useEffect(() => {
      if (isPreview) {
        setStats({
          total: 1234,
          game: 120,
          video: 400,
          music: 300,
          anime: 200,
          tv_series: 164,
          book: 50,
        })
        setLoading(false)
        return
      }

      // 先从缓存加载
      const hasCache = loadFromCache()
      if (hasCache) {
        setLoading(false)
      }

      // 然后获取最新数据
      fetchLibraryStats()
    }, [loadFromCache, fetchLibraryStats, isPreview])

    const categories = useMemo(
      () => [
        { key: 'game', label: t.quickStats.game, color: '#1b2838' },
        { key: 'video', label: t.quickStats.video, color: '#00A1D6' },
        { key: 'music', label: t.quickStats.music, color: '#d33a31' },
        { key: 'anime', label: t.quickStats.anime, color: '#fb7299' },
        { key: 'tv_series', label: t.quickStats.tvSeries, color: '#6366f1' },
        { key: 'book', label: t.quickStats.book, color: '#059669' },
      ],
      [t],
    )

    // 有数据的分类才显示；加载中或资料库为空时显示全部，避免空白
    const visibleCategories = useMemo(() => {
      const withData = categories.filter(
        (cat) => ((stats as any)[cat.key] ?? 0) > 0,
      )
      return withData.length > 0 ? withData : categories
    }, [categories, stats])

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={12}
        contentClassName="flex flex-col"
        background={
          <GlowBackground
            color="var(--color-primary)"
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="md"
          />
        }
      >
        <div className="relative flex min-h-0 flex-1 flex-col">
          {/* 顶部：标题 + 总数 */}
          <div
            className="flex items-start justify-between mb-2 ml-1.5"
            style={{
              marginBottom: `${8 * scale}px`,
              marginLeft: `${6 * scale}px`,
            }}
          >
            <div>
              <h3
                className="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold mb-0.5"
                style={{
                  fontSize: `${12 * fontScale}px`,
                  marginBottom: `${2 * scale}px`,
                }}
              >
                {t.quickStats.widgetTitle}
              </h3>
              <motion.div
                className="flex items-baseline gap-1"
                style={{ gap: `${4 * scale}px` }}
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{ duration: 0.4, delay: 0.1 }}
              >
                <span
                  className="text-3xl font-black text-gray-800 dark:text-gray-100 leading-none"
                  style={{ fontSize: `${30 * fontScale}px` }}
                >
                  {stats.total}
                </span>
                <span
                  className="text-xs text-gray-500 dark:text-gray-400 font-bold mb-0.5"
                  style={{
                    fontSize: `${12 * fontScale}px`,
                    marginBottom: `${2 * scale}px`,
                  }}
                >
                  ITEMS
                </span>
              </motion.div>
            </div>
          </div>

          {/* 分类统计 */}
          <div
            className="flex-1 grid gap-1.5"
            style={{
              gap: `${6 * scale}px`,
              gridTemplateColumns: `repeat(${visibleCategories.length}, minmax(0, 1fr))`,
            }}
          >
            {visibleCategories.map((cat, index) => (
              <motion.div
                key={cat.key}
                initial={{ y: 10, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                transition={{ duration: 0.3, delay: 0.2 + index * 0.05 }}
              >
                <StatCard
                  cat={cat}
                  value={(stats as any)[cat.key]}
                  loading={false}
                  index={index}
                  scale={scale}
                  fontScale={fontScale}
                />
              </motion.div>
            ))}
          </div>

          <WidgetSkeletonCover
            active={loading}
            preset="stats-grid"
            count={4}
            accent="var(--color-primary)"
            label={t.common.loading}
          />
        </div>
      </WidgetShell>
    )
  },
)

QuickStatsWidget.displayName = 'QuickStatsWidget'
