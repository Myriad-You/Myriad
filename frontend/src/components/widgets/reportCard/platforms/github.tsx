import type { LangSegment } from '../types'
import { LuGitFork, LuStar } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useMemo } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import {
  HEATMAP_DAYS,
  HEATMAP_WEEKS,
} from '../animations'
import { useLibraryItemRotation } from '../hooks'

// GitHub组件（完整版）
export const GithubStatsWidget = memo(({ data }: any) => {
  const { t } = useI18n()
  const langs = useMemo(() => data?.languages || [], [data?.languages])
  // 语言构成条：模仿 Bangumi 类型占比设计，各色段首尾相接连续填充
  const langSegments = useMemo<LangSegment[]>(() => {
    const items = langs
      .filter((l: any) => l.percentage > 0)
      .sort((a: any, b: any) => b.percentage - a.percentage)
      .slice(0, 4)
    const total = items.reduce((sum: number, l: any) => sum + l.percentage, 0)
    if (total === 0) return []
    const fillDuration = 0.9
    const baseDelay = 0.55
    let acc = 0
    return items.map((lang: any) => {
      const segment: LangSegment = {
        name: lang.name as string,
        pct: (lang.percentage / total) * 100,
        delay: baseDelay + (acc / total) * fillDuration,
        duration: (lang.percentage / total) * fillDuration,
      }
      acc += lang.percentage
      return segment
    })
  }, [langs])
  /** BE enum: legendary|veteran|active|emerging (+ legacy Chinese normalized on read). */
  const levelKey = useMemo((): 'legendary' | 'veteran' | 'active' | 'emerging' => {
    const raw = String(data?.contribution_level || '')
      .trim()
      .toLowerCase()
    if (
      raw === 'legendary' ||
      raw.includes('传奇') ||
      raw.includes('legendary')
    ) {
      return 'legendary'
    }
    if (
      raw === 'veteran' ||
      raw.includes('资深工程') ||
      raw.includes('资深开发') ||
      raw.includes('veteran')
    ) {
      return 'veteran'
    }
    if (
      raw === 'active' ||
      raw.includes('活跃') ||
      raw.includes('高级') ||
      raw.includes('中级') ||
      raw.includes('senior') ||
      raw.includes('intermediate')
    ) {
      return 'active'
    }
    return 'emerging'
  }, [data?.contribution_level])

  const levelLabel = useMemo(() => {
    switch (levelKey) {
      case 'legendary':
        return t.reportCardWidget.legendaryDev
      case 'veteran':
        return t.reportCardWidget.veteranDev
      case 'active':
        return t.reportCardWidget.activeDev
      default:
        return t.reportCardWidget.beginnerDev
    }
  }, [levelKey, t])

  const safeNonNegInt = (v: unknown): number => {
    const n = Number(v)
    if (!Number.isFinite(n) || n < 0) return 0
    return Math.round(n)
  }
  const contributions = useMemo(
    () => safeNonNegInt(data?.total_contributions),
    [data?.total_contributions],
  )
  const reposCount = useMemo(
    () => safeNonNegInt(data?.repos_count),
    [data?.repos_count],
  )
  const totalStars = useMemo(
    () => safeNonNegInt(data?.total_stars),
    [data?.total_stars],
  )
  const contributionCalendar = useMemo(
    () => data?.contribution_calendar,
    [data?.contribution_calendar],
  )

  const levelColor = useMemo(() => {
    const colorMap: Record<string, string> = {
      emerging: '#22c55e',
      active: '#3b82f6',
      veteran: '#f97316',
      legendary: '#ef4444',
    }
    return colorMap[levelKey] || '#6b7280'
  }, [levelKey])

  /**
   * GitHub-style week columns (Sun–Sat rows) for the last N complete weeks.
   * When calendar data is missing, show empty cells (no random fake counts).
   */
  const generateHeatmapGrid = () => {
    const WEEKS = 12
    const DAYS = 7
    const grid: Array<{
      week: number
      day: number
      opacity: number
      count: number
    }> = []

    const byDate = new Map<string, number>()
    if (contributionCalendar && Array.isArray(contributionCalendar)) {
      for (const d of contributionCalendar) {
        const date =
          typeof d?.date === 'string'
            ? d.date.slice(0, 10)
            : typeof d?.day === 'string'
              ? d.day.slice(0, 10)
              : ''
        if (!date) continue
        byDate.set(date, Number(d?.count) || 0)
      }
    }

    const maxCount = Math.max(1, ...Array.from(byDate.values()), 1)
    // End on most recent Sunday-aligned week ending today (UTC date string)
    const today = new Date()
    const end = new Date(
      Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate()),
    )
    // Align end to end of current week (Saturday = 6 in UTC getUTCDay)
    const endDow = end.getUTCDay() // 0 Sun … 6 Sat
    // Grid: columns = weeks, rows = Sun(0)…Sat(6)
    const totalCells = WEEKS * DAYS
    const start = new Date(end)
    start.setUTCDate(start.getUTCDate() - (totalCells - 1) + (6 - endDow))

    for (let week = 0; week < WEEKS; week++) {
      for (let day = 0; day < DAYS; day++) {
        const cellDate = new Date(start)
        cellDate.setUTCDate(start.getUTCDate() + week * 7 + day)
        const key = cellDate.toISOString().slice(0, 10)
        const count = byDate.get(key) ?? 0
        const opacity =
          byDate.size === 0
            ? 0.08
            : count > 0
              ? Math.min((count / maxCount) * 0.85 + 0.15, 1)
              : 0.12
        grid.push({ week, day, opacity, count })
      }
    }
    return grid
  }

  const heatmapData = useMemo(
    () => generateHeatmapGrid(),
    [contributionCalendar],
  )

  const getLanguageColor = (lang: string) => {
    const colorMap: { [key: string]: string } = {
      TypeScript: '#3178c6',
      JavaScript: '#f1e05a',
      Python: '#3572A5',
      Rust: '#dea584',
      Go: '#00ADD8',
      Java: '#b07219',
      'C++': '#f34b7d',
      'C#': '#178600',
      Ruby: '#701516',
      PHP: '#4F5D95',
    }
    return colorMap[lang] || levelColor
  }

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="relative h-full flex flex-col p-2 justify-between">
        <div className="space-y-2">
          <div className="flex items-start justify-between">
            <div className="flex flex-col gap-2 items-start">
              <motion.div
                className="px-2 py-0.5 rounded-md text-[9px] font-bold flex items-center gap-1 shadow-sm w-fit"
                style={{
                  backgroundColor: `${levelColor}20`,
                  color: levelColor,
                  border: `1px solid ${levelColor}30`,
                }}
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{ duration: 0.3, delay: 0.2 }}
              >
                <span className="text-[7px]">●</span>
                <span>{levelLabel}</span>
              </motion.div>
              <div className="flex flex-col gap-1.5">
                <motion.div
                  className="flex items-baseline gap-1.5"
                  initial={{ y: 10, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.3 }}
                >
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-100 leading-none">
                    {contributions}
                  </span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">
                    {t.reportsPage.commits}
                  </span>
                </motion.div>
                <motion.div
                  className="flex items-baseline gap-1.5"
                  initial={{ y: 10, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.4 }}
                >
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-100 leading-none">
                    {reposCount}
                  </span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">
                    {t.reportsPage.repos}
                  </span>
                </motion.div>
              </div>
            </div>
            <div className="flex flex-col items-end gap-1.5">
              <div className="flex gap-[2.5px]">
                {HEATMAP_WEEKS.map((week) => (
                  <div key={week} className="flex flex-col gap-[2.5px]">
                    {HEATMAP_DAYS.map((day) => {
                      // heatmapData: week*7+day (Sun–Sat), real calendar not random
                      const cell = heatmapData[week * 7 + day]
                      return (
                        <motion.div
                          key={`${week}-${day}`}
                          className="w-2 h-2 rounded-0.5"
                          style={{
                            backgroundColor: levelColor,
                            opacity: cell?.opacity || 0.15,
                          }}
                          initial={{ scale: 0, opacity: 0 }}
                          animate={{ scale: 1, opacity: cell?.opacity || 0.15 }}
                          transition={{
                            duration: 0.2,
                            delay: (week * 7 + day) * 0.003,
                          }}
                        />
                      )
                    })}
                  </div>
                ))}
              </div>
              {totalStars > 0 && (
                <motion.div
                  className="px-1.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1"
                  style={{
                    backgroundColor: `${levelColor}1a`,
                    color: levelColor,
                  }}
                  initial={{ scale: 0.8, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  transition={{ duration: 0.3, delay: 0.5 }}
                >
                  <LuStar size={9} />
                  <span>
                    {totalStars >= 1000
                      ? `${(totalStars / 1000).toFixed(1)}k`
                      : totalStars}
                  </span>
                </motion.div>
              )}
            </div>
          </div>
        </div>
        {langSegments.length > 0 && (
          <div className="absolute bottom-3 right-3 w-[45%] flex flex-col items-end gap-1">
            {/* 首页 4x2 更窄时 flex-wrap 易折到 3 行；硬限制最多两行，多余裁切 */}
            <div
              className="flex max-h-[1.375rem] flex-wrap content-start justify-end gap-x-2.5 gap-y-0.5 overflow-hidden"
              title={langSegments
                .map((s) => `${s.name} ${Math.round(s.pct)}%`)
                .join(' · ')}
            >
              {langSegments.map((segment) => (
                <motion.span
                  key={segment.name}
                  className="flex items-center gap-1 text-[8px] font-bold leading-none text-gray-600 dark:text-gray-300"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.3, delay: segment.delay }}
                >
                  <span
                    className="h-1.5 w-1.5 shrink-0 rounded-full"
                    style={{ backgroundColor: getLanguageColor(segment.name) }}
                  />
                  {segment.name}
                  <span className="font-mono text-gray-500 dark:text-gray-400">
                    {Math.round(segment.pct)}%
                  </span>
                </motion.span>
              ))}
            </div>
            <div className="flex h-1 w-full rounded-full overflow-hidden bg-gray-200/80 dark:bg-white/10 ring-1 ring-black/5 dark:ring-white/10">
              {langSegments.map((segment) => (
                <motion.div
                  key={segment.name}
                  className="h-full"
                  style={{ backgroundColor: getLanguageColor(segment.name) }}
                  initial={{ width: 0 }}
                  animate={{ width: `${segment.pct}%` }}
                  transition={{
                    duration: segment.duration,
                    delay: segment.delay,
                    ease: 'linear',
                  }}
                />
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  )
})

export const GithubWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(
    () => data?.library_items || [],
    [data?.library_items],
  )
  const { currentItem, currentItemIndex } = useLibraryItemRotation(
    libraryItems,
    showOverview,
  )

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({
        title: currentItem.title,
        type: currentItem.language || 'repo',
      })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem || libraryItems.length === 0 ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <GithubStatsWidget data={data} />
        </motion.div>
      ) : (
        <motion.div
          key={`lib-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          <div className="relative h-full w-full rounded-lg overflow-hidden shadow-lg bg-white dark:bg-black/90">
            <div className="absolute inset-0 bg-linear-to-br from-gray-800 to-gray-900 dark:from-black dark:to-black/90">
              <div className="absolute inset-0">
                {currentItem.description && (
                  <div className="p-2.5 pr-3">
                    <p
                      className="overflow-hidden text-[11px] font-medium leading-[1.4] break-words text-gray-200"
                      style={{
                        display: '-webkit-box',
                        WebkitBoxOrient: 'vertical',
                        WebkitLineClamp: 5,
                        maxHeight: 'calc(1.4em * 5)',
                      }}
                    >
                      {currentItem.description}
                    </p>
                  </div>
                )}
                {(currentItem.stars !== undefined ||
                  currentItem.forks !== undefined) && (
                  <div className="absolute right-1.5 bottom-1.5 z-10 flex h-8 items-center gap-1.5">
                    {currentItem.stars !== undefined && (
                      <div className="flex items-center gap-1 rounded bg-gray-700/50 px-1.5 py-0.5">
                        <LuStar size={11} className="text-amber-400" />
                        <span className="text-[11px] font-bold text-gray-100">
                          {currentItem.stars >= 1000
                            ? `${(currentItem.stars / 1000).toFixed(1)}k`
                            : currentItem.stars}
                        </span>
                      </div>
                    )}
                    {currentItem.forks !== undefined && (
                      <div className="flex items-center gap-1 rounded bg-gray-700/50 px-1.5 py-0.5">
                        <LuGitFork size={11} className="text-gray-100" />
                        <span className="text-[11px] font-bold text-gray-100">
                          {currentItem.forks >= 1000
                            ? `${(currentItem.forks / 1000).toFixed(1)}k`
                            : currentItem.forks}
                        </span>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})
