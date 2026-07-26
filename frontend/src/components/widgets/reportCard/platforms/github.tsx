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

// ==================== GitHub组件（完整版）====================
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
  const level = useMemo(
    () => data?.contribution_level || t.reportCard.beginnerDev,
    [data?.contribution_level, t.reportCard.beginnerDev],
  )
  const contributions = useMemo(
    () => data?.total_contributions || 0,
    [data?.total_contributions],
  )
  const reposCount = useMemo(() => data?.repos_count || 0, [data?.repos_count])
  const totalStars = useMemo(() => data?.total_stars || 0, [data?.total_stars])
  const contributionCalendar = useMemo(
    () => data?.contribution_calendar,
    [data?.contribution_calendar],
  )

  const levelColor = useMemo(() => {
    const colorMap: { [key: string]: string } = {
      [t.reportCardWidget.beginnerDev]: '#22c55e',
      [t.reportCardWidget.intermediateDev]: '#3b82f6',
      [t.reportCardWidget.seniorDev]: '#a855f7',
      [t.reportCardWidget.veteranDev]: '#f97316',
      [t.reportCardWidget.legendaryDev]: '#ef4444',
    }
    return colorMap[level] || '#6b7280'
  }, [level, t])

  const generateHeatmapGrid = () => {
    const grid: Array<{
      week: number
      day: number
      opacity: number
      count: number
    }> = []

    if (contributionCalendar && Array.isArray(contributionCalendar)) {
      const recentDays = contributionCalendar.slice(-60)
      const maxCount = Math.max(...recentDays.map((d: any) => d.count || 0), 1)

      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const index = week * 5 + day
          const dayData = recentDays[index]
          const count = dayData?.count || 0
          const opacity =
            count > 0 ? Math.min((count / maxCount) * 0.85 + 0.15, 1) : 0.12
          grid.push({ week, day, opacity, count })
        }
      }
    } else {
      const avgPerDay = contributions / 365
      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const lambda = avgPerDay * (0.5 + Math.random())
          const count = Math.floor(-Math.log(1 - Math.random()) * lambda)
          const opacity =
            count > 0
              ? Math.min((count / (avgPerDay * 2)) * 0.7 + 0.15, 1)
              : 0.12
          grid.push({ week, day, opacity, count })
        }
      }
    }
    return grid
  }

  const heatmapData = useMemo(
    () => generateHeatmapGrid(),
    [contributionCalendar, contributions],
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
                <span>{level}</span>
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
                      // heatmapData 按 week*5+day 顺序生成，直接下标取，避免 O(n²) find
                      const cell = heatmapData[week * 5 + day]
                      return (
                        <motion.div
                          key={`${week}-${day}`}
                          className="w-2.5 h-2.5 rounded-0.5"
                          style={{
                            backgroundColor: levelColor,
                            opacity: cell?.opacity || 0.15,
                          }}
                          initial={{ scale: 0, opacity: 0 }}
                          animate={{ scale: 1, opacity: cell?.opacity || 0.15 }}
                          transition={{
                            duration: 0.2,
                            delay: (week * 5 + day) * 0.004,
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
          <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
            <div className="absolute inset-0 bg-linear-to-br from-gray-800 to-gray-900 dark:from-black dark:to-black/90">
              <div className="absolute inset-0 flex flex-col p-2.5 pb-[20%]">
                <div className="flex items-center gap-2.5 mb-2">
                  {currentItem.stars !== undefined && (
                    <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-700/50">
                      <LuStar size={10} className="text-amber-400" />
                      <span className="text-[10px] font-bold text-gray-100">
                        {currentItem.stars >= 1000
                          ? `${(currentItem.stars / 1000).toFixed(1)}k`
                          : currentItem.stars}
                      </span>
                    </div>
                  )}
                  {currentItem.forks !== undefined && (
                    <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-700/50">
                      <LuGitFork size={10} className="text-gray-100" />
                      <span className="text-[10px] font-bold text-gray-100">
                        {currentItem.forks >= 1000
                          ? `${(currentItem.forks / 1000).toFixed(1)}k`
                          : currentItem.forks}
                      </span>
                    </div>
                  )}
                </div>
                {currentItem.description && (
                  <div className="text-[10px] leading-snug text-gray-200 line-clamp-4 px-1">
                    {currentItem.description}
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
