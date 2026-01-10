import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { memo, useEffect, useId, useMemo, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { useLoopAnimation } from '../hooks/animation'
import { useReportsVisibilityInterval } from '../hooks/animation/pages/reports'
import { useAnimationLevel } from '../hooks/useAnimationLevel'

// 🔧 性能优化：预生成热力图网格索引，避免在渲染时调用 Array.from
const HEATMAP_WEEKS = Array.from({ length: 12 }, (_, i) => i)
const HEATMAP_DAYS = Array.from({ length: 5 }, (_, i) => i)

// 🔧 工具函数：处理B站图片URL，使用后端代理
export function getBilibiliProxyUrl(cover?: string, title?: string): string {
  if (!cover) {
    return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || 'B')}&size=400&background=00A1D6&color=fff`
  }
  if (cover.startsWith('/api/proxy/')) {
    return cover
  }
  if (cover.includes('hdslb.com') || cover.includes('bilibili.com')) {
    return `${API_URL || ''}/api/proxy/image?url=${encodeURIComponent(cover)}`
  }
  return cover
}

// 迷你组件：B站弹幕云
export const DanmakuWidget = memo(({ data }: { data?: { danmaku?: string[] } }) => {
  const { t } = useI18n()
  const texts = useMemo(() => data?.danmaku || t.reportsPage.danmakuDefaults, [data?.danmaku, t.reportsPage.danmakuDefaults])
  const anim = useAnimationLevel()
  const uniqueId = useId()

  // 使用触发式循环动画（弹幕核心动画）
  const { isAnimating } = useLoopAnimation({
    duration: 11000, // 弹幕滚动约8秒 + 额外保持3秒
    enabled: anim.loop,
  })

  // 🆕 低性能模式：限制弹幕数量不超过3条
  const maxDanmakuCount = anim.loop ? (Math.random() < 0.7 ? (Math.random() < 0.5 ? 3 : 4) : 5) : 3

  const animations = useMemo(() => {
    const lanes = 5
    const usedLanes: number[] = []

    return texts.slice(0, maxDanmakuCount).map((_, i) => {
      let lane: number
      do {
        lane = Math.floor(Math.random() * lanes)
      } while (usedLanes.includes(lane))
      usedLanes.push(lane)

      return {
        duration: 6 + Math.random() * 4,
        delay: i * 0.7 + Math.random() * 0.5,
        top: `${10 + lane * 18}%`,
        opacity: 0.4 + Math.random() * 0.3,
      }
    })
  }, [texts, maxDanmakuCount])

  return (
    <div className="relative h-full w-full overflow-hidden">
      {animations.map((a, i) => (
        <motion.div
          key={`${texts[i]}-${i}`}
          initial={{ x: '100%', opacity: 0 }}
          animate={{
            x: '-100%',
            opacity: [0, 1, 1, 0],
          }}
          transition={{
            repeat: 0, // 只运行一轮，由调度器控制重新播放
            duration: a.duration,
            delay: a.delay,
            ease: 'linear',
          }}
          className="absolute whitespace-nowrap text-base font-bold danmaku-text-color gpu-accelerated"
          style={{
            top: a.top,
            opacity: a.opacity,
          }}
        >
          {texts[i]}
        </motion.div>
      ))}
    </div>
  )
})

// B站综合展示组件
export const BilibiliWidget = memo(({ data, onContentChange, showOverview }: {
  data?: { danmaku?: string[], library_items?: Array<{ title: string, cover?: string, type: string }> }
  onContentChange?: (item: { title: string, type: string } | null) => void
  showOverview: boolean
}) => {
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])

  // 🔧 使用报告页原子化可见性感知定时器
  useReportsVisibilityInterval(
    () => setCurrentItemIndex(prev => (prev + 1) % libraryItems.length),
    !showOverview && libraryItems.length > 0 ? 5000 : null,
  )

  const currentItem = libraryItems[currentItemIndex]

  useEffect(() => {
    if (!showOverview && currentItem && libraryItems.length > 0) {
      onContentChange?.({ title: currentItem.title, type: currentItem.type })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange, libraryItems.length])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem || libraryItems.length === 0
        ? (
            <motion.div
              key="danmaku"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full"
            >
              <DanmakuWidget data={data} />
            </motion.div>
          )
        : (
            <motion.div
              key={`library-${currentItemIndex}`}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full p-1.5"
            >
              <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                <div className="absolute inset-0">
                  <img
                    src={getBilibiliProxyUrl(currentItem.cover, currentItem.title)}
                    alt={currentItem.title}
                    className="w-full h-full object-cover"
                    loading="lazy"
                  />
                  <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
                </div>
              </div>
            </motion.div>
          )}
    </AnimatePresence>
  )
})

// Steam统计展示
export const SteamStatsWidget = memo(({ data }: { data?: {
  hardcore_score?: number
  player_type?: string
  games_count?: number
  total_playtime?: number
} }) => {
  const { t } = useI18n()
  const score = useMemo(() => data?.hardcore_score || 0, [data?.hardcore_score])
  const type = useMemo(() => data?.player_type || t.reportsPage.casualPlayer, [data?.player_type, t.reportsPage.casualPlayer])
  const gamesCount = useMemo(() => data?.games_count || 0, [data?.games_count])
  const totalPlaytime = useMemo(() => {
    const hours = data?.total_playtime || 0
    if (hours >= 1000)
      return `${(hours / 1000).toFixed(1)}k`
    return hours.toString()
  }, [data?.total_playtime])

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0">
        <div className="absolute inset-0 bg-gradient-to-br from-gray-100/50 to-transparent dark:from-white/[0.02] dark:to-transparent clip-diagonal" />
      </div>

      <motion.div
        className="absolute top-2 left-4 z-10"
        initial={{ y: -20, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        transition={{ duration: 0.6, delay: 0.1 }}
      >
        <div className="flex items-start gap-1">
          <motion.span
            className="text-5xl font-black text-gray-800 dark:text-gray-100 leading-none"
            initial={{ scale: 0.5 }}
            animate={{ scale: 1 }}
            transition={{ duration: 0.5, delay: 0.3, type: 'spring', stiffness: 200 }}
          >
            {score}
          </motion.span>
          <span className="text-xs text-gray-500 dark:text-gray-400 font-bold mt-1">/100</span>
        </div>

        <motion.div
          className="mt-2"
          initial={{ y: 10, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          transition={{ duration: 0.5, delay: 0.5 }}
        >
          <div className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-gray-800/90 dark:bg-white/90 backdrop-blur-sm">
            <div className="w-1.5 h-1.5 rounded-full bg-gray-300 dark:bg-black/60 animate-pulse" />
            <span className="text-[10px] font-bold text-gray-100 dark:text-black uppercase tracking-wide">{type}</span>
          </div>
        </motion.div>
      </motion.div>

      <div className="absolute right-0 top-0 bottom-0 w-1/3 flex flex-col justify-center items-end pr-5 gap-4">
        <motion.div
          className="flex flex-col items-end"
          initial={{ x: 30, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          transition={{ duration: 0.5, delay: 0.4 }}
        >
          <div className="flex flex-col items-end">
            <span className="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-widest font-bold">{t.reportsPage.library}</span>
            <span className="text-3xl font-black text-gray-800 dark:text-gray-200 leading-none">{gamesCount}</span>
          </div>
        </motion.div>

        <motion.div
          className="flex flex-col items-end"
          initial={{ x: 30, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          transition={{ duration: 0.5, delay: 0.6 }}
        >
          <div className="flex flex-col items-end">
            <span className="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-widest font-bold">{t.reportsPage.playtime}</span>
            <div className="flex items-baseline gap-0.5">
              <span className="text-3xl font-black text-gray-800 dark:text-gray-200 leading-none">{totalPlaytime}</span>
              <span className="text-[10px] text-gray-600 dark:text-gray-400 font-bold mb-1">H</span>
            </div>
          </div>
        </motion.div>
      </div>
    </div>
  )
})

// Steam综合展示组件
export const SteamWidget = memo(({ data, onContentChange, showOverview }: {
  data?: {
    hardcore_score?: number
    player_type?: string
    games_count?: number
    total_playtime?: number
    library_items?: Array<{ title: string, cover?: string, type: string }>
  }
  onContentChange?: (item: { title: string, type: string } | null) => void
  showOverview: boolean
}) => {
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])

  // 🔧 使用报告页原子化可见性感知定时器
  useReportsVisibilityInterval(
    () => setCurrentItemIndex(prev => (prev + 1) % libraryItems.length),
    !showOverview && libraryItems.length > 0 ? 5000 : null,
  )

  const currentItem = libraryItems[currentItemIndex]

  useEffect(() => {
    if (!showOverview && currentItem && libraryItems.length > 0) {
      onContentChange?.({ title: currentItem.title, type: currentItem.type })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange, libraryItems.length])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem || libraryItems.length === 0
        ? (
            <motion.div
              key="stats"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full"
            >
              <SteamStatsWidget data={data} />
            </motion.div>
          )
        : (
            <motion.div
              key={`library-${currentItemIndex}`}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full p-1.5"
            >
              <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                <div className="absolute inset-0">
                  <img
                    src={currentItem.cover || `https://ui-avatars.com/api/?name=${encodeURIComponent(currentItem.title)}&size=400&background=1b2838&color=fff`}
                    alt={currentItem.title}
                    className="w-full h-full object-cover"
                    loading="lazy"
                  />
                  <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
                </div>
              </div>
            </motion.div>
          )}
    </AnimatePresence>
  )
})

// GitHub统计展示
export const GithubStatsWidget = memo(({ data }: { data?: {
  contribution_level?: string
  total_contributions?: number
  repos_count?: number
  languages?: { name: string, percentage: number }[]
  contribution_calendar?: Array<{ date: string, count: number }>
} }) => {
  const { t } = useI18n()
  const level = useMemo(() => data?.contribution_level || t.reportsPage.activeDeveloper, [data?.contribution_level, t.reportsPage.activeDeveloper])
  const contributions = useMemo(() => data?.total_contributions || 0, [data?.total_contributions])
  const reposCount = useMemo(() => data?.repos_count || 0, [data?.repos_count])
  const langs = useMemo(() => data?.languages || [], [data?.languages])
  const contributionCalendar = useMemo(() => data?.contribution_calendar || [], [data?.contribution_calendar])

  const getLevelColor = (level: string) => {
    // 使用翻译的关键词进行匹配
    const legendaryKeyword = t.reportsPage.legendary
    const coreKeyword = t.reportsPage.core
    const seniorKeyword = t.reportsPage.senior
    const prolificKeyword = t.reportsPage.prolific
    const activeKeyword = t.reportsPage.active

    if (level.includes(legendaryKeyword) || level.includes(coreKeyword))
      return '#22c55e'
    if (level.includes(seniorKeyword) || level.includes(prolificKeyword))
      return '#3b82f6'
    if (level.includes(activeKeyword))
      return '#8b5cf6'
    return '#6b7280'
  }

  const levelColor = getLevelColor(level)

  const generateHeatmapGrid = () => {
    const grid = []
    if (contributionCalendar.length > 0) {
      const recentDays = contributionCalendar.slice(-60)
      const maxCount = Math.max(...recentDays.map(d => d.count), 1)
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
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-200 leading-none">{contributions}</span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">{t.reportsPage.commits}</span>
                </motion.div>
                <motion.div
                  className="flex items-baseline gap-1.5"
                  initial={{ y: 10, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.4 }}
                >
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
                        style={{
                          backgroundColor: levelColor,
                          opacity: cell?.opacity || 0.15,
                        }}
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
          {langs.slice(0, 2).map((lang, i) => (
            <motion.div
              key={i}
              className="relative pl-12"
              initial={{ x: -20, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.5 + i * 0.1 }}
            >
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

// GitHub综合展示组件
export const GithubWidget = memo(({ data, onContentChange, showOverview }: {
  data?: {
    languages?: { name: string, percentage: number }[]
    contribution_level?: string
    total_contributions?: number
    repos_count?: number
    library_items?: Array<{
      title: string
      language?: string
      type: string
      stars?: number
      forks?: number
      description?: string
    }>
  }
  onContentChange?: (item: { title: string, type: string } | null) => void
  showOverview: boolean
}) => {
  const { t } = useI18n()
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])

  // 🔧 使用报告页原子化可见性感知定时器
  useReportsVisibilityInterval(
    () => setCurrentItemIndex(prev => (prev + 1) % libraryItems.length),
    !showOverview && libraryItems.length > 0 ? 5000 : null,
  )

  useEffect(() => {
    if (showOverview && libraryItems.length > 0) {
      setCurrentItemIndex(prev => (prev + 1) % libraryItems.length)
    }
  }, [showOverview, libraryItems.length])

  const currentItem = libraryItems[currentItemIndex]

  useEffect(() => {
    if (!showOverview && currentItem && libraryItems.length > 0) {
      onContentChange?.({ title: currentItem.title, type: currentItem.language || 'repo' })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange, libraryItems.length])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem || libraryItems.length === 0
        ? (
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
          )
        : (
            <motion.div
              key={`library-${currentItemIndex}`}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full p-1.5"
            >
              {currentItem
                ? (
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
                  )
                : (
                    <div className="h-full w-full flex items-center justify-center">
                      <div className="text-[9px] font-mono text-gray-400 text-center">
                        {t.reportsPage.noReposFound}
                      </div>
                    </div>
                  )}
            </motion.div>
          )}
    </AnimatePresence>
  )
})

// 网易云音乐卡片展示
export const MusicStatsWidget = memo(({ data }: { data?: {
  soul_color?: string
  mood_keywords?: Array<{ tag: string, color: string }>
  follower_count?: number
  playlist_count?: number
  level?: number
} }) => {
  const { t } = useI18n()
  const animConfig = useAnimationLevel()
  const uniqueId = useId()

  // 使用触发式循环动画（音乐气泡核心动画）
  const { isAnimating } = useLoopAnimation({
    duration: 5000, // 气泡浮动约5秒周期
    enabled: animConfig.loop,
  })

  const canAnimate = animConfig.loop && isAnimating

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
    const items: Array<{
      tag: string
      color: string
      x: number
      y: number
      size: number
      floatDuration: number
      floatDelay: number
    }> = []

    const hash = (str: string, seed: number) => {
      let h = seed
      for (let j = 0; j < str.length; j++) {
        h = Math.imul(h ^ str.charCodeAt(j), 2654435761)
      }
      return ((h ^ (h >>> 16)) >>> 0) / 4294967296
    }

    const isOverlappingStats = (x: number, y: number) => {
      return x > 60 && y > 60
    }

    moodKeywords.forEach((keyword, i) => {
      const size = 35 + Math.floor(hash(keyword.tag, 1) * 60)
      let bestX = 50
      let bestY = 50
      let maxMinDist = -1

      for (let attempt = 0; attempt < 30; attempt++) {
        const r1 = hash(keyword.tag, 100 + attempt + i * 50)
        const r2 = hash(keyword.tag, 200 + attempt + i * 50)
        const x = 10 + r1 * 80
        const y = 10 + r2 * 80

        if (isOverlappingStats(x, y))
          continue

        let minDist = 1000
        if (items.length === 0) {
          minDist = 1000
        }
        else {
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
              }}
              initial={{ scale: 0, opacity: 0 }}
              animate={{
                scale: 1,
                opacity: 1,
                y: canAnimate ? [0, -8, 0, 8, 0] : 0,
                boxShadow: `
                  0 8px 20px -6px ${bubble.color}60, 
                  inset 0 4px 10px rgba(255,255,255,0.3),
                  inset 0 -5px 15px ${bubble.color}30
                `,
                zIndex: 10,
              }}
              transition={{
                scale: { type: 'spring', stiffness: 260, damping: 20, delay: i * 0.1 },
                opacity: { duration: 0.6, delay: i * 0.1 },
                y: canAnimate ? {
                  duration: bubble.floatDuration,
                  repeat: 2, // 有限次数
                  ease: 'easeInOut',
                  delay: bubble.floatDelay,
                } : { duration: 0.3 },
                boxShadow: { duration: 0.3, ease: 'easeInOut' },
                zIndex: { delay: 0.1 },
              }}
              whileHover={{
                scale: 1.15,
                zIndex: 50,
                boxShadow: `0 15px 35px -5px ${bubble.color}80, inset 0 0 20px rgba(255,255,255,0.6)`,
                transition: {
                  duration: 0.3,
                  ease: 'easeOut',
                },
              }}
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
              <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                {formatNumber(followerCount)}
              </span>
              <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">
                {t.reportsPage.fans}
              </span>
            </div>
            <div className="w-px h-5 bg-gray-300 dark:bg-white/20" />
            <div className="flex flex-col items-end">
              <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                {formatNumber(playlistCount)}
              </span>
              <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">
                {t.reportsPage.lists}
              </span>
            </div>
          </motion.div>
        </div>
      </div>
    </div>
  )
})

// 网易云综合展示组件
export const NeteaseWidget = memo(({
  data,
  onContentChange,
  showOverview,
}: {
  data?: any
  onContentChange?: (item: { titles: string[], type: string } | null) => void
  showOverview: boolean
}) => {
  const processedData = useMemo(() => {
    if (!data)
      return undefined
    let moodKeywords: Array<{ tag: string, color: string }> = []
    if (data.mood_keywords) {
      if (Array.isArray(data.mood_keywords)) {
        if (data.mood_keywords.length > 0) {
          if (typeof data.mood_keywords[0] === 'string') {
            const defaultColors = ['#7B68EE', '#FF6B9D', '#4ECDC4', '#FFB347', '#95E1D3']
            moodKeywords = data.mood_keywords.map((tag: string, i: number) => ({
              tag,
              color: defaultColors[i % defaultColors.length],
            }))
          }
          else if (typeof data.mood_keywords[0] === 'object') {
            moodKeywords = data.mood_keywords
          }
        }
      }
    }

    return {
      soul_color: data.soul_color,
      mood_keywords: moodKeywords,
      library_items: data.library_items,
      follower_count: data.follower_count,
      playlist_count: data.playlist_count,
      level: data.level,
    }
  }, [data])

  const libraryItems = useMemo(() => processedData?.library_items || [], [processedData?.library_items])
  const [currentItemIndex, setCurrentItemIndex] = useState(0)

  // 🔧 使用报告页原子化可见性感知定时器（网易云每次跳2首）
  useReportsVisibilityInterval(
    () => setCurrentItemIndex(prev => (prev + 2) % libraryItems.length),
    !showOverview && libraryItems.length > 0 ? 5000 : null,
  )

  const currentItems = [
    libraryItems[currentItemIndex],
    libraryItems[(currentItemIndex + 1) % libraryItems.length],
  ].filter(Boolean)

  useEffect(() => {
    if (!showOverview && currentItems.length > 0 && libraryItems.length > 0) {
      onContentChange?.({
        titles: currentItems.map(item => item.title),
        type: 'music',
      })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItems, onContentChange, libraryItems.length])

  return (
    <AnimatePresence mode="wait">
      {showOverview || currentItems.length === 0 || libraryItems.length === 0
        ? (
            <motion.div
              key="stats"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full"
            >
              <MusicStatsWidget data={processedData} />
            </motion.div>
          )
        : (
            <motion.div
              key={`music-${currentItemIndex}`}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.5 }}
              className="h-full w-full p-1.5"
            >
              <div className="h-full w-full flex gap-1.5">
                {currentItems.map((item, idx) => (
                  <div key={idx} className="flex-1 h-full">
                    <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                      <div className="absolute inset-0">
                        <img
                          src={item.cover || `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=200&background=e60026&color=fff`}
                          alt={item.title}
                          className="w-full h-full object-cover"
                          loading="lazy"
                        />
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
