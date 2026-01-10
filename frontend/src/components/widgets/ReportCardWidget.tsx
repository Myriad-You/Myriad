/**
 * 报告页平台卡片小组件 - 完整版（非阉割）
 * 完全复用 Reports.tsx 中的所有子组件实现
 */

import type { WidgetConfig } from '../WidgetGrid'
import { FaGithub, FaSteam, SiBilibili, SiNeteasecloudmusic } from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { getLatestReportDeduped } from '../../utils/requestDedup'

// 🔧 性能优化：预生成热力图网格索引，避免在渲染时调用 Array.from
const HEATMAP_WEEKS = Array.from({ length: 12 }, (_, i) => i)
const HEATMAP_DAYS = Array.from({ length: 5 }, (_, i) => i)
const LANES_ARRAY = Array.from({ length: 5 }, (_, i) => i)

// ==================== 静态动画常量（避免每次渲染创建新对象）====================
// 弹幕动画 - 有限次数，配合调度器 duration=11000ms
const DANMAKU_INITIAL = { x: '100%', opacity: 0 }
const DANMAKU_ANIMATE = { x: '-100%', opacity: [0, 1, 1, 0] }
function createDanmakuTransition(duration: number, delay: number) {
  return {
    repeat: 0, // 只运行一轮，由调度器控制重新播放
    duration,
    delay,
    ease: 'linear' as const,
  }
}

// 内容切换动画
const CONTENT_FADE_INITIAL = { opacity: 0 }
const CONTENT_FADE_ANIMATE = { opacity: 1 }
const CONTENT_FADE_EXIT = { opacity: 0 }
const CONTENT_FADE_TRANSITION = { duration: 0.5 }

const CONTENT_SLIDE_INITIAL = { opacity: 0, y: 10 }
const CONTENT_SLIDE_ANIMATE = { opacity: 1, y: 0 }
const CONTENT_SLIDE_EXIT = { opacity: 0, y: -10 }
const CONTENT_SLIDE_TRANSITION = { duration: 0.5 }

export interface ReportCardWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
}

// ==================== 工具函数 ====================
function getBilibiliProxyUrl(cover?: string, title?: string): string {
  if (!cover) {
    return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || 'B')}&size=400&background=00A1D6&color=fff`
  }
  if (cover.startsWith('/api/proxy/'))
    return cover
  if (cover.includes('hdslb.com') || cover.includes('bilibili.com')) {
    return `${API_URL || ''}/api/proxy/image?url=${encodeURIComponent(cover)}`
  }
  return cover
}

function useLibraryItemRotation(libraryItems: any[], showOverview: boolean) {
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const prevShowOverviewRef = useRef(showOverview)

  useEffect(() => {
    // 当从概览模式切换到库项目模式时，更新索引
    if (prevShowOverviewRef.current && !showOverview && libraryItems.length > 0) {
      setCurrentItemIndex(prev => (prev + 1) % libraryItems.length)
    }
    prevShowOverviewRef.current = showOverview
  }, [showOverview, libraryItems.length])

  return { currentItem: libraryItems[currentItemIndex], currentItemIndex }
}

// ==================== B站组件（完整版）====================
const DanmakuWidget = memo(({ data, allowLoop = true, triggerKey }: { data?: { danmaku?: string[] }, allowLoop?: boolean, triggerKey?: unknown }) => {
  const { t } = useI18n()
  const defaultDanmaku = t.reportCard.danmakuDefault as unknown as string[]
  const texts = useMemo(() => data?.danmaku || defaultDanmaku, [data?.danmaku, defaultDanmaku])
  const uniqueId = useId()

  // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
  const { isAnimating } = useLoopAnimation({
    duration: 11000, // 弹幕滚动约8秒 + 额外保持3秒
    trigger: triggerKey, // 状态切换时触发
    enabled: allowLoop, // 低端设备禁用
  })

  // 🆕 低性能模式：限制弹幕数量不超过3条
  const maxDanmakuCount = allowLoop ? (Math.random() < 0.7 ? (Math.random() < 0.5 ? 3 : 4) : 5) : 3

  const animations = useMemo(() => {
    // 🔧 使用预生成的 LANES_ARRAY 进行洗牌
    const availableLanes = [...LANES_ARRAY]
    for (let i = availableLanes.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [availableLanes[i], availableLanes[j]] = [availableLanes[j], availableLanes[i]]
    }
    return texts.slice(0, maxDanmakuCount).map((_, i) => ({
      duration: 6 + Math.random() * 4,
      delay: i * 0.7 + Math.random() * 0.5,
      top: `${10 + availableLanes[i] * 18}%`,
      opacity: 0.4 + Math.random() * 0.3,
    }))
  }, [texts, maxDanmakuCount])

  return (
    <div className="relative h-full w-full overflow-hidden">
      {animations.map((anim, i) => (
        <motion.div
          key={`${texts[i]}-${i}`}
          initial={DANMAKU_INITIAL}
          animate={DANMAKU_ANIMATE}
          transition={createDanmakuTransition(anim.duration, anim.delay)}
          className="absolute whitespace-nowrap text-base font-bold danmaku-text-color gpu-accelerated"
          style={{
            top: anim.top,
            opacity: anim.opacity,
          }}
        >
          {texts[i]}
        </motion.div>
      ))}
    </div>
  )
})
DanmakuWidget.displayName = 'DanmakuWidget'

const BilibiliWidget = memo(({ data, showOverview, onContentChange, allowLoop = true }: any) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(libraryItems, showOverview)

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: currentItem.type })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem
        ? (
            <motion.div key="danmaku" initial={CONTENT_FADE_INITIAL} animate={CONTENT_FADE_ANIMATE} exit={CONTENT_FADE_EXIT} transition={CONTENT_FADE_TRANSITION} className="h-full w-full">
              <DanmakuWidget data={data} allowLoop={allowLoop} triggerKey={showOverview} />
            </motion.div>
          )
        : (
            <motion.div key={`lib-${currentItemIndex}`} initial={CONTENT_SLIDE_INITIAL} animate={CONTENT_SLIDE_ANIMATE} exit={CONTENT_SLIDE_EXIT} transition={CONTENT_SLIDE_TRANSITION} className="h-full w-full p-1.5">
              <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                <div className="absolute inset-0">
                  <img src={getBilibiliProxyUrl(currentItem.cover, currentItem.title)} alt={currentItem.title} className="w-full h-full object-cover" loading="lazy" />
                  <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
                </div>
              </div>
            </motion.div>
          )}
    </AnimatePresence>
  )
})

// ==================== Steam组件（完整版）====================
const SteamStatsWidget = memo(({ data }: any) => {
  const { t } = useI18n()
  const score = useMemo(() => data?.hardcore_score || 0, [data])
  const type = useMemo(() => data?.player_type || t.reportCard.casualPlayer, [data, t.reportCard.casualPlayer])
  const gamesCount = useMemo(() => data?.games_count || 0, [data])
  const totalPlaytime = useMemo(() => {
    const hours = data?.total_playtime || 0
    return hours >= 1000 ? `${(hours / 1000).toFixed(1)}k` : hours.toString()
  }, [data])

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0">
        <div className="absolute inset-0 bg-gradient-to-br from-gray-100/50 to-transparent dark:from-white/[0.02] dark:to-transparent clip-diagonal" />
      </div>
      <motion.div className="absolute top-2 left-4 z-10" initial={{ y: -20, opacity: 0 }} animate={{ y: 0, opacity: 1 }} transition={{ duration: 0.6, delay: 0.1 }}>
        <div className="flex items-start gap-1">
          <motion.span className="text-5xl font-black text-gray-800 dark:text-gray-100 leading-none" initial={{ scale: 0.5 }} animate={{ scale: 1 }} transition={{ duration: 0.5, delay: 0.3, type: 'spring', stiffness: 200 }}>{score}</motion.span>
          <span className="text-xs text-gray-500 dark:text-gray-400 font-bold mt-1">/100</span>
        </div>
        <motion.div className="mt-2" initial={{ y: 10, opacity: 0 }} animate={{ y: 0, opacity: 1 }} transition={{ duration: 0.5, delay: 0.5 }}>
          <div className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-gray-800/90 dark:bg-white/90 backdrop-blur-sm">
            <div className="w-1.5 h-1.5 rounded-full bg-gray-300 dark:bg-black/60 animate-pulse" />
            <span className="text-[10px] font-bold text-gray-100 dark:text-black uppercase tracking-wide">{type}</span>
          </div>
        </motion.div>
      </motion.div>
      <div className="absolute right-0 top-0 bottom-0 w-1/3 flex flex-col justify-center items-end pr-5 gap-4">
        <motion.div className="flex flex-col items-end" initial={{ x: 30, opacity: 0 }} animate={{ x: 0, opacity: 1 }} transition={{ duration: 0.5, delay: 0.4 }}>
          <span className="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-widest font-bold">{t.reportsPage.library}</span>
          <span className="text-3xl font-black text-gray-800 dark:text-gray-200 leading-none">{gamesCount}</span>
        </motion.div>
        <motion.div className="flex flex-col items-end" initial={{ x: 30, opacity: 0 }} animate={{ x: 0, opacity: 1 }} transition={{ duration: 0.5, delay: 0.6 }}>
          <span className="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-widest font-bold">{t.reportsPage.playtime}</span>
          <div className="flex items-baseline gap-0.5">
            <span className="text-3xl font-black text-gray-800 dark:text-gray-200 leading-none">{totalPlaytime}</span>
            <span className="text-[10px] text-gray-600 dark:text-gray-400 font-bold mb-1">H</span>
          </div>
        </motion.div>
      </div>
    </div>
  )
})

const SteamWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(libraryItems, showOverview)

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: 'game' })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem
        ? (
            <motion.div key="stats" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.5 }} className="h-full w-full">
              <SteamStatsWidget data={data} />
            </motion.div>
          )
        : (
            <motion.div key={`lib-${currentItemIndex}`} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -10 }} transition={{ duration: 0.5 }} className="h-full w-full p-1.5">
              <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                <div className="absolute inset-0">
                  <img src={currentItem.cover || `https://ui-avatars.com/api/?name=${encodeURIComponent(currentItem.title)}&size=400&background=1b2838&color=fff`} alt={currentItem.title} className="w-full h-full object-cover" loading="lazy" />
                  <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
                </div>
              </div>
            </motion.div>
          )}
    </AnimatePresence>
  )
})

// ==================== GitHub组件（完整版）====================
const GithubStatsWidget = memo(({ data }: any) => {
  const { t } = useI18n()
  const langs = useMemo(() => data?.languages || [], [data?.languages])
  const level = useMemo(() => data?.contribution_level || t.reportCard.beginnerDev, [data?.contribution_level, t.reportCard.beginnerDev])
  const contributions = useMemo(() => data?.total_contributions || 0, [data?.total_contributions])
  const reposCount = useMemo(() => data?.repos_count || 0, [data?.repos_count])
  const contributionCalendar = useMemo(() => data?.contribution_calendar, [data?.contribution_calendar])

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
    const grid: Array<{ week: number, day: number, opacity: number, count: number }> = []

    if (contributionCalendar && Array.isArray(contributionCalendar)) {
      const recentDays = contributionCalendar.slice(-60)
      const maxCount = Math.max(...recentDays.map((d: any) => d.count || 0), 1)

      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const index = week * 5 + day
          const dayData = recentDays[index]
          const count = dayData?.count || 0
          const opacity = count > 0 ? Math.min((count / maxCount) * 0.85 + 0.15, 1) : 0.12
          grid.push({ week, day, opacity, count })
        }
      }
    }
    else {
      const avgPerDay = contributions / 365
      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const lambda = avgPerDay * (0.5 + Math.random())
          const count = Math.floor(-Math.log(1 - Math.random()) * lambda)
          const opacity = count > 0 ? Math.min(count / (avgPerDay * 2) * 0.7 + 0.15, 1) : 0.12
          grid.push({ week, day, opacity, count })
        }
      }
    }
    return grid
  }

  const heatmapData = useMemo(() => generateHeatmapGrid(), [contributionCalendar, contributions])

  const getLanguageColor = (lang: string) => {
    const colorMap: { [key: string]: string } = {
      'TypeScript': '#3178c6',
      'JavaScript': '#f1e05a',
      'Python': '#3572A5',
      'Rust': '#dea584',
      'Go': '#00ADD8',
      'Java': '#b07219',
      'C++': '#f34b7d',
      'C#': '#178600',
      'Ruby': '#701516',
      'PHP': '#4F5D95',
    }
    return colorMap[lang] || levelColor
  }

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-gray-50/50 to-transparent dark:from-white/[0.02] dark:to-transparent" />
      <div className="relative h-full flex flex-col p-2 justify-between">
        <div className="space-y-2">
          <div className="flex items-start justify-between">
            <div className="flex flex-col gap-2 items-start">
              <motion.div
                className="px-2 py-0.5 rounded-md text-[9px] font-bold flex items-center gap-1 shadow-sm w-fit"
                style={{ backgroundColor: `${levelColor}20`, color: levelColor, border: `1px solid ${levelColor}30` }}
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{ duration: 0.3, delay: 0.2 }}
              >
                <span className="text-[7px]">●</span>
                <span>{level}</span>
              </motion.div>
              <div className="flex flex-col gap-1.5">
                <motion.div className="flex items-baseline gap-1.5" initial={{ y: 10, opacity: 0 }} animate={{ y: 0, opacity: 1 }} transition={{ duration: 0.4, delay: 0.3 }}>
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-200 leading-none">{contributions}</span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">{t.reportsPage.commits}</span>
                </motion.div>
                <motion.div className="flex items-baseline gap-1.5" initial={{ y: 10, opacity: 0 }} animate={{ y: 0, opacity: 1 }} transition={{ duration: 0.4, delay: 0.4 }}>
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-200 leading-none">{reposCount}</span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">{t.reportsPage.repos}</span>
                </motion.div>
              </div>
            </div>
            <div className="flex gap-[2.5px]">
              {HEATMAP_WEEKS.map(week => (
                <div key={week} className="flex flex-col gap-[2.5px]">
                  {HEATMAP_DAYS.map((day) => {
                    const cell = heatmapData.find(c => c.week === week && c.day === day)
                    return (
                      <motion.div
                        key={`${week}-${day}`}
                        className="w-[10px] h-[10px] rounded-[2px]"
                        style={{ backgroundColor: levelColor, opacity: cell?.opacity || 0.15 }}
                        initial={{ scale: 0, opacity: 0 }}
                        animate={{ scale: 1, opacity: cell?.opacity || 0.15 }}
                        transition={{ duration: 0.2, delay: (week * 5 + day) * 0.004 }}
                      />
                    )
                  })}
                </div>
              ))}
            </div>
          </div>
        </div>
        <div className="space-y-1 pb-1">
          {langs.slice(0, 2).map((lang: any, i: number) => (
            <motion.div key={i} className="relative pl-12" initial={{ x: -20, opacity: 0 }} animate={{ x: 0, opacity: 1 }} transition={{ duration: 0.4, delay: 0.5 + i * 0.1 }}>
              <div className="flex items-center justify-between mb-0.5">
                <span className="text-[8px] font-bold text-gray-700 dark:text-gray-300">{lang.name}</span>
                <span className="text-[7px] font-mono text-gray-500 dark:text-gray-400">
                  {lang.percentage}
                  %
                </span>
              </div>
              <div className="h-[3px] w-full bg-gray-200 dark:bg-white/5 rounded-full overflow-hidden">
                <motion.div
                  className="h-full rounded-full"
                  style={{ backgroundColor: getLanguageColor(lang.name) }}
                  initial={{ width: 0 }}
                  animate={{ width: `${lang.percentage}%` }}
                  transition={{ duration: 0.8, delay: 0.6 + i * 0.1, ease: 'easeOut' }}
                />
              </div>
            </motion.div>
          ))}
        </div>
      </div>
    </div>
  )
})

const GithubWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(libraryItems, showOverview)

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: currentItem.language || 'repo' })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem || libraryItems.length === 0
        ? (
            <motion.div key="stats" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.5 }} className="h-full w-full">
              <GithubStatsWidget data={data} />
            </motion.div>
          )
        : (
            <motion.div key={`lib-${currentItemIndex}`} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -10 }} transition={{ duration: 0.5 }} className="h-full w-full p-1.5">
              <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                <div className="absolute inset-0 bg-gradient-to-br from-gray-800 to-gray-900 dark:from-black dark:to-black/90">
                  <div className="absolute inset-0 flex flex-col p-2.5 pb-[20%]">
                    <div className="flex items-center gap-2.5 mb-2">
                      {currentItem.stars !== undefined && (
                        <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-700/50">
                          <span className="text-xs">⭐</span>
                          <span className="text-[10px] font-bold text-gray-100">
                            {currentItem.stars >= 1000 ? `${(currentItem.stars / 1000).toFixed(1)}k` : currentItem.stars}
                          </span>
                        </div>
                      )}
                      {currentItem.forks !== undefined && (
                        <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-700/50">
                          <span className="text-xs">🍴</span>
                          <span className="text-[10px] font-bold text-gray-100">
                            {currentItem.forks >= 1000 ? `${(currentItem.forks / 1000).toFixed(1)}k` : currentItem.forks}
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

// ==================== Netease组件（完整版）====================
const MusicStatsWidget = memo(({ data, allowLoop = true, triggerKey }: { data?: any, allowLoop?: boolean, triggerKey?: unknown }) => {
  const { t } = useI18n()
  const uniqueId = useId()

  // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
  const { isAnimating } = useLoopAnimation({
    duration: 5000, // 气泡动画约5秒
    trigger: triggerKey, // 状态切换时触发
    enabled: allowLoop, // 低端设备禁用
  })

  const canAnimate = allowLoop && isAnimating

  const color = useMemo(() => data?.soul_color || '#ef4444', [data?.soul_color])
  const moodKeywords = useMemo(() => data?.mood_keywords || [], [data?.mood_keywords])
  const followerCount = useMemo(() => data?.follower_count || 0, [data?.follower_count])
  const playlistCount = useMemo(() => data?.playlist_count || 0, [data?.playlist_count])
  const level = useMemo(() => data?.level || 0, [data?.level])

  const formatNumber = (num: number) => {
    if (num >= 10000)
      return `${(num / 10000).toFixed(1)}${t.reportsPage.tenThousandSuffix}`
    if (num >= 1000)
      return `${(num / 1000).toFixed(1)}k`
    return num.toString()
  }

  const bubbles = useMemo(() => {
    const items: Array<{ tag: string, color: string, x: number, y: number, size: number, floatDuration: number, floatDelay: number }> = []
    const hash = (str: string, seed: number) => {
      let h = seed
      for (let j = 0; j < str.length; j++) {
        h = Math.imul(h ^ str.charCodeAt(j), 2654435761)
      }
      return ((h ^ (h >>> 16)) >>> 0) / 4294967296
    }
    const isOverlappingStats = (x: number, y: number) => x > 60 && y > 60

    moodKeywords.forEach((keyword: any, i: number) => {
      const size = 35 + Math.floor(hash(keyword.tag, 1) * 60)
      let bestX = 50; let bestY = 50; let maxMinDist = -1

      for (let attempt = 0; attempt < 30; attempt++) {
        const r1 = hash(keyword.tag, 100 + attempt + i * 50)
        const r2 = hash(keyword.tag, 200 + attempt + i * 50)
        const x = 10 + r1 * 80
        const y = 10 + r2 * 80
        if (isOverlappingStats(x, y))
          continue

        let minDist = 1000
        if (items.length > 0) {
          for (const item of items) {
            const dx = x - item.x
            const dy = (y - item.y) * 2
            const d = Math.sqrt(dx * dx + dy * dy)
            if (d < minDist)
              minDist = d
          }
        }
        if (minDist > maxMinDist) {
          maxMinDist = minDist
          bestX = x
          bestY = y
        }
      }

      items.push({
        tag: keyword.tag,
        color: keyword.color,
        x: bestX,
        y: bestY,
        size,
        floatDuration: 3 + hash(keyword.tag, 4) * 4,
        floatDelay: hash(keyword.tag, 5) * 2,
      })
    })
    return items
  }, [moodKeywords])

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-red-50/50 to-transparent dark:from-red-900/20 dark:to-transparent" />
      <div className="relative h-full w-full p-3">
        <div className="absolute inset-0 pointer-events-none">
          {bubbles.map((bubble, i) => (
            <motion.div
              key={bubble.tag}
              className="absolute flex items-center justify-center rounded-full font-bold backdrop-blur-[1px] pointer-events-auto cursor-default"
              style={{
                left: `${bubble.x}%`,
                top: `${bubble.y}%`,
                width: `${bubble.size}px`,
                height: `${bubble.size}px`,
                marginLeft: `-${bubble.size / 2}px`,
                marginTop: `-${bubble.size / 2}px`,
                background: `radial-gradient(120% 120% at 30% 30%, rgba(255,255,255,0.6) 0%, ${bubble.color}20 20%, ${bubble.color}60 100%)`,
                border: `1px solid rgba(255,255,255,0.3)`,
                color: bubble.color,
                fontSize: `${Math.min(Math.max(10, bubble.size / 4), 16)}px`,
                textShadow: `0 1px 1px rgba(255,255,255,0.8)`,
                zIndex: 10,
                willChange: 'transform', // GPU 加速
                transform: 'translateZ(0)',
                backfaceVisibility: 'hidden',
              }}
              initial={{ scale: 0, opacity: 0 }}
              animate={{
                scale: 1,
                opacity: 1,
                y: [0, -8, 0, 8, 0],
                // 移除动态 boxShadow 动画，使用静态样式代替
              }}
              transition={{
                scale: { type: 'spring', stiffness: 260, damping: 20, delay: i * 0.1 },
                opacity: { duration: 0.6, delay: i * 0.1 },
                y: { duration: bubble.floatDuration, repeat: canAnimate ? Infinity : 0, ease: 'easeInOut', delay: bubble.floatDelay },
              }}
              whileHover={{ scale: 1.15, zIndex: 50, transition: { duration: 0.3, ease: 'easeOut' } }}
            >
              <div className="absolute top-[15%] left-[15%] w-[20%] h-[10%] bg-white/30 rounded-full blur-[1px] transform -rotate-45" />
              <span className="relative z-10 mix-blend-multiply dark:mix-blend-normal">{bubble.tag}</span>
            </motion.div>
          ))}
        </div>
        <div className="absolute bottom-3 right-3 flex flex-col items-end gap-2 z-20">
          <motion.div
            className="px-2.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1 backdrop-blur-md shadow-lg bg-gradient-to-br from-red-50 to-red-100 dark:from-red-950/80 dark:to-red-900/60 text-red-600 dark:text-red-300"
            style={{ boxShadow: '0 2px 12px rgba(239, 68, 68, 0.25)' }}
            initial={{ scale: 0.8, opacity: 0, x: 20 }}
            animate={{ scale: 1, opacity: 1, x: 0 }}
            transition={{ duration: 0.4, delay: 0.1 }}
          >
            <span className="text-[7px]">●</span>
            <span>
              Lv.
              {level}
            </span>
          </motion.div>
          <motion.div
            className="flex items-center gap-2 px-3 py-1.5 rounded-xl backdrop-blur-md shadow-lg bg-white/90 dark:bg-black/90 border border-white/30 dark:border-white/10"
            style={{ backdropFilter: 'blur(10px)' }}
            initial={{ scale: 0.8, opacity: 0, x: 20 }}
            animate={{ scale: 1, opacity: 1, x: 0 }}
            transition={{ duration: 0.4, delay: 0.2 }}
          >
            <div className="flex flex-col items-end">
              <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">{formatNumber(followerCount)}</span>
              <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">{t.reportsPage.fans}</span>
            </div>
            <div className="w-px h-5 bg-gray-300 dark:bg-white/20" />
            <div className="flex flex-col items-end">
              <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">{formatNumber(playlistCount)}</span>
              <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">{t.reportsPage.lists}</span>
            </div>
          </motion.div>
        </div>
      </div>
    </div>
  )
})

const NeteaseWidget = memo(({ data, showOverview, onContentChange, allowLoop = true }: any) => {
  const processedData = useMemo(() => {
    if (!data)
      return undefined
    let moodKeywords: Array<{ tag: string, color: string }> = []
    if (data.mood_keywords && Array.isArray(data.mood_keywords)) {
      if (data.mood_keywords.length > 0) {
        if (typeof data.mood_keywords[0] === 'string') {
          const defaultColors = ['#7B68EE', '#FF6B9D', '#4ECDC4', '#FFB347', '#95E1D3']
          moodKeywords = data.mood_keywords.map((tag: string, i: number) => ({ tag, color: defaultColors[i % defaultColors.length] }))
        }
        else if (typeof data.mood_keywords[0] === 'object') {
          moodKeywords = data.mood_keywords
        }
      }
    }
    return { soul_color: data.soul_color, mood_keywords: moodKeywords, library_items: data.library_items, follower_count: data.follower_count, playlist_count: data.playlist_count, level: data.level }
  }, [data])

  const libraryItems = useMemo(() => processedData?.library_items || [], [processedData?.library_items])
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const prevShowOverviewRef = useRef(showOverview)

  // 当从概览切换到库项目模式时，立即更新索引
  useEffect(() => {
    if (prevShowOverviewRef.current && !showOverview && libraryItems.length > 0) {
      setCurrentItemIndex(prev => (prev + 2) % libraryItems.length)
    }
    prevShowOverviewRef.current = showOverview
  }, [showOverview, libraryItems.length])

  // 在非概览模式下，定时轮换项目 - timeout 链 + 可见性暂停
  useEffect(() => {
    if (!showOverview && libraryItems.length > 0) {
      let cancelled = false
      let timeoutId: number | null = null
      const tick = () => {
        if (cancelled || document.hidden)
          return
        setCurrentItemIndex(prev => (prev + 2) % libraryItems.length)
        timeoutId = window.setTimeout(tick, 5000)
      }
      timeoutId = window.setTimeout(tick, 5000)

      const onVisibility = () => {
        if (document.hidden && timeoutId) {
          clearTimeout(timeoutId)
          timeoutId = null
        }
        else if (!document.hidden && !cancelled && !timeoutId) {
          tick()
        }
      }
      document.addEventListener('visibilitychange', onVisibility)

      return () => {
        cancelled = true
        if (timeoutId)
          clearTimeout(timeoutId)
        document.removeEventListener('visibilitychange', onVisibility)
      }
    }
  }, [showOverview, libraryItems.length])

  const currentItems = useMemo(() => [libraryItems[currentItemIndex], libraryItems[(currentItemIndex + 1) % libraryItems.length]].filter(Boolean), [libraryItems, currentItemIndex])

  useEffect(() => {
    if (!showOverview && currentItems.length > 0) {
      onContentChange?.({ titles: currentItems.map((item: any) => item.title), type: 'music' })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItems, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || currentItems.length === 0 || libraryItems.length === 0
        ? (
            <motion.div key="stats" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.5 }} className="h-full w-full">
              <MusicStatsWidget data={processedData} allowLoop={allowLoop} triggerKey={showOverview} />
            </motion.div>
          )
        : (
            <motion.div key={`music-${currentItemIndex}`} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -10 }} transition={{ duration: 0.5 }} className="h-full w-full p-1.5">
              <div className="h-full w-full flex gap-1.5">
                {currentItems.map((item: any, idx: number) => (
                  <div key={idx} className="flex-1 h-full">
                    <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                      <div className="absolute inset-0">
                        <img src={item.cover || `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=200&background=e60026&color=fff`} alt={item.title} className="w-full h-full object-cover" loading="lazy" />
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </motion.div>
          )}
    </AnimatePresence>
  )
})

// ==================== 平台配置 ====================
const PLATFORM_CONFIG: Record<string, { icon: React.ReactNode, color: string, bgColor: string, borderColor: string, label: string, textColor: string }> = {
  bilibili: { icon: <SiBilibili />, color: '#00A1D6', bgColor: 'rgba(0, 161, 214, 0.15)', borderColor: 'rgba(0, 161, 214, 0.3)', label: 'Bilibili', textColor: 'text-[#00A1D6]' },
  steam: { icon: <FaSteam />, color: '#1b2838', bgColor: 'rgba(27, 40, 56, 0.15)', borderColor: 'rgba(27, 40, 56, 0.3)', label: 'Steam', textColor: 'text-gray-700 dark:text-gray-300' },
  github: { icon: <FaGithub />, color: '#24292e', bgColor: 'rgba(36, 41, 46, 0.15)', borderColor: 'rgba(36, 41, 46, 0.3)', label: 'GitHub', textColor: 'text-gray-900 dark:text-gray-100' },
  netease: { icon: <SiNeteasecloudmusic />, color: '#e60026', bgColor: 'rgba(230, 0, 38, 0.15)', borderColor: 'rgba(230, 0, 38, 0.3)', label: 'NetEase', textColor: 'text-red-600' },
}

// ==================== 主组件 ====================
export const ReportCardWidget = memo(({ config, isEditMode, isPreview }: ReportCardWidgetProps) => {
  const animLevel = useAnimationLevel()
  const { t } = useI18n()
  const platformId = (config.config?.platformId || 'bilibili') as string
  const [reportData, setReportData] = useState<any>(null)
  const [loading, setLoading] = useState(true)
  const [showOverview, setShowOverview] = useState(true)
  const [cardContent, setCardContent] = useState<{ title: string, type?: string, titles?: string[] } | null>(null)

  // 翻译的平台标签
  const translatedPlatformLabels = useMemo(() => ({
    bilibili: t.reportCardWidget.bilibili,
    netease: t.reportCardWidget.neteaseMusic,
  }), [t])

  useEffect(() => {
    if (isPreview) {
      setReportData({
        hardcore_score: 85,
        player_type: t.reportCardWidget.hardcorePlayer,
        games_count: 120,
        total_playtime: 2500,
        contribution_level: t.reportCardWidget.seniorDev,
        total_contributions: 1200,
        repos_count: 45,
        follower_count: 1200,
        playlist_count: 15,
        level: 8,
        mood_keywords: [t.reportCardWidget.happyMood, t.reportCardWidget.sadMood, t.reportCardWidget.passionateMood],
        library_items: [
          { title: t.reportCardWidget.sampleProject, type: 'repo', stars: 120, forks: 30, description: t.reportCardWidget.sampleProjectDesc },
          { title: t.reportCardWidget.sampleGame, type: 'game', cover: '' },
          { title: t.reportCardWidget.sampleAnime, type: 'anime', cover: '' },
          { title: t.reportCardWidget.samplePlaylist, type: 'music', cover: '' },
        ],
      })
      setLoading(false)
      return
    }

    const fetchReport = async () => {
      try {
        // 使用去重机制避免多个 ReportCardWidget 同时请求
        const data = await getLatestReportDeduped()
        const report = data.platform_reports?.find((r: any) => r.platform === platformId)
        if (report)
          setReportData(report.card_visuals)
      }
      catch (err) {
        console.error(`${t.reportCardWidget.fetchReportFailed}:`, err)
      }
      finally {
        setLoading(false)
      }
    }
    fetchReport()

    // 5分钟刷新一次 - timeout 链 + 可见性暂停
    let cancelled = false
    let timeoutId: number | null = null
    const schedule = () => {
      if (cancelled || document.hidden)
        return
      fetchReport()
      timeoutId = window.setTimeout(schedule, 5 * 60 * 1000)
    }
    timeoutId = window.setTimeout(schedule, 5 * 60 * 1000)

    const onVisibility = () => {
      if (document.hidden && timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      }
      else if (!document.hidden && !cancelled && !timeoutId) {
        schedule()
      }
    }
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      cancelled = true
      if (timeoutId)
        clearTimeout(timeoutId)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [platformId])

  useEffect(() => {
    if (isPreview)
      return

    // 10秒切换概览/详情 - timeout 链 + 可见性暂停
    let cancelled = false
    let timeoutId: number | null = null
    const tick = () => {
      if (cancelled || document.hidden)
        return
      setShowOverview(prev => !prev)
      timeoutId = window.setTimeout(tick, 10000)
    }
    timeoutId = window.setTimeout(tick, 10000)

    const onVisibility = () => {
      if (document.hidden && timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      }
      else if (!document.hidden && !cancelled && !timeoutId) {
        tick()
      }
    }
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      cancelled = true
      if (timeoutId)
        clearTimeout(timeoutId)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [isPreview])

  const handleContentChange = useCallback((content: any) => {
    setCardContent(content)
  }, [])

  if (loading)
    return <div className="h-full w-full flex items-center justify-center"><div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" /></div>
  if (!reportData)
    return <div className="h-full w-full flex items-center justify-center text-gray-400 text-sm"><span>{t.reportCard.noReportData}</span></div>

  const platformConfig = PLATFORM_CONFIG[platformId] || PLATFORM_CONFIG.bilibili

  return (
    <div className="relative h-full w-full rounded-xl overflow-hidden glass">
      {/* 动态背景光效 */}
      <div
        className={`absolute -right-10 -top-10 w-40 h-40 rounded-full ${animLevel.level === 'standard' ? 'blur-3xl' : 'blur-xl'} opacity-10 group-hover:opacity-20 transition-opacity`}
        style={{ background: platformConfig.color }}
      />

      {/* 主内容区 */}
      <div className="absolute inset-0 flex flex-col z-10">
        {platformId === 'bilibili' && <BilibiliWidget data={reportData} showOverview={showOverview} onContentChange={handleContentChange} allowLoop={animLevel.loop} />}
        {platformId === 'steam' && <SteamWidget data={reportData} showOverview={showOverview} onContentChange={handleContentChange} />}
        {platformId === 'github' && <GithubWidget data={reportData} showOverview={showOverview} onContentChange={handleContentChange} />}
        {platformId === 'netease' && <NeteaseWidget data={reportData} showOverview={showOverview} onContentChange={handleContentChange} allowLoop={animLevel.loop} />}
      </div>

      {/* 左下角浮动Logo */}
      <motion.div
        className="absolute bottom-3 left-3 z-20"
        initial={false}
        animate={{ width: cardContent ? 'auto' : '32px' }}
        transition={{ duration: 0.3, ease: 'easeOut' }}
      >
        <div
          className={`rounded-lg flex items-center gap-2 ${platformConfig.textColor} backdrop-blur-sm shadow-lg transition-all overflow-hidden ${
            cardContent
              ? 'bg-white/95 dark:bg-black/95'
              : ''
          }`}
          style={{
            background: cardContent ? undefined : platformConfig.bgColor,
            border: `1px solid ${platformConfig.borderColor}`,
            padding: cardContent ? (platformId === 'netease' && cardContent.titles ? '4px 8px' : '0 8px') : '0 8px',
            height: cardContent ? (platformId === 'netease' && cardContent.titles ? 'auto' : '32px') : '32px',
          }}
        >
          <div className={`text-base flex-shrink-0 ${platformConfig.textColor}`}>{platformConfig.icon}</div>
          <AnimatePresence>
            {cardContent && (
              <motion.div
                initial={{ opacity: 0, width: 0 }}
                animate={{ opacity: 1, width: 'auto' }}
                exit={{ opacity: 0, width: 0 }}
                transition={{ duration: 0.3 }}
                className="flex items-center gap-2 whitespace-nowrap overflow-hidden"
              >
                {platformId === 'netease' && cardContent.titles
                  ? (
                      <div className="flex flex-col gap-0.5">
                        {cardContent.titles.map((title: string, idx: number) => (
                          <div key={idx} className="text-[10px] font-bold text-gray-900 dark:text-gray-100 max-w-[120px] truncate leading-tight">
                            {title}
                          </div>
                        ))}
                      </div>
                    )
                  : (
                      <span className="text-[11px] font-bold text-gray-900 dark:text-gray-100 max-w-[120px] truncate">
                        {cardContent.title}
                      </span>
                    )}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </motion.div>
    </div>
  )
})

ReportCardWidget.displayName = 'ReportCardWidget'
