import type { SecondaryNavItem } from '../contexts/NavigationContext'
import {
  FaChartPie,
  FaGithub,
  FaMagic,
  FaRobot,
  FaSteam,
  FaSync,
  FaTimes,
  SiBilibili,
  SiNeteasecloudmusic,
} from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import AnimatedView from '../components/AnimatedView'
import StageMode from '../components/StageMode'
import Toast from '../components/Toast'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useSecondaryNav } from '../contexts/NavigationContext'
import { useLoopAnimation, usePageReady } from '../hooks/animation'
import { useReportsScheduler, useReportsVisibilityInterval } from '../hooks/animation/pages/reports'
import { useAnimationLevel } from '../hooks/useAnimationLevel'
import { useTitleFont } from '../hooks/useTitleFont'
import { getCSRFToken } from '../utils/csrf'
import { hasSessionHint } from '../utils/sessionDetection'
import { ComprehensiveReportCard } from './reports/ComprehensiveReportCard'
import { EmptyComprehensiveReport } from './reports/EmptyComprehensiveReport'

// 🔧 性能优化：预生成热力图网格索引，避免在渲染时调用 Array.from
const HEATMAP_WEEKS = Array.from({ length: 12 }, (_, i) => i)
const HEATMAP_DAYS = Array.from({ length: 5 }, (_, i) => i)

// 🚀 性能优化：防抖Hook
function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value)

  useEffect(() => {
    const handler = setTimeout(() => {
      setDebouncedValue(value)
    }, delay)

    return () => {
      clearTimeout(handler)
    }
  }, [value, delay])

  return debouncedValue
}

// 🚀 性能优化：共享的资料库项目切换Hook
function useLibraryItemRotation(libraryItems: any[], showOverview: boolean) {
  const [currentItemIndex, setCurrentItemIndex] = useState(0)

  // 当切换回概览模式时，更新下一个要显示的项目的索引
  useEffect(() => {
    if (showOverview && libraryItems.length > 0) {
      setCurrentItemIndex(prev => (prev + 1) % libraryItems.length)
    }
  }, [showOverview, libraryItems.length])

  const currentItem = libraryItems[currentItemIndex]

  return { currentItem, currentItemIndex }
}

// 🚀 性能优化：骨架屏加载组件
const SkeletonCard = memo(() => (
  <div className="relative aspect-[2/1] rounded-2xl overflow-hidden glass animate-pulse">
    <div className="absolute inset-0 p-3.5 flex flex-col justify-between">
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-1.5">
          <div className="w-4 h-4 bg-gray-300 dark:bg-neutral-800 rounded" />
          <div className="w-16 h-3 bg-gray-300 dark:bg-neutral-800 rounded" />
        </div>
      </div>
      <div className="flex-1 flex items-center justify-center">
        <div className="w-20 h-20 bg-gray-300 dark:bg-neutral-800 rounded-full" />
      </div>
    </div>
  </div>
))

interface PlatformReport {
  platform: string
  metadata: any
  summary: string
  insights: string[]
  card_visuals?: {
    danmaku?: string[]
    player_type?: string
    hardcore_score?: number
    top_genres?: string[]
    contribution_level?: string
    languages?: { name: string, percentage: number }[]
    soul_color?: string
    mood_keywords?: string[]
  }
  created_at: string
}

interface BackgroundElement {
  type: 'circle' | 'rect' | 'gradient' | 'pattern' | 'svg'
  style?: React.CSSProperties
  className?: string
  animate?: any // framer-motion animate props
  transition?: any // framer-motion transition props
  svgPath?: string // SVG path data
  content?: string // 文本或emoji内容
}

interface ComprehensiveAnalysis {
  // AI自由生成的内容字段 - 使用索引签名接受任意字段
  [key: string]: any

  // 必需的样式字段（用于前端渲染）
  theme_color: string
  visual_style: string
  decorative_emojis: string[]
  card_subtitle: string
  key_metric: string
  background_elements?: BackgroundElement[]

  // 图标字段（三选一，优先级从高到低）
  icon_image_url?: string // AI生成的图标URL或图片链接
  icon_prompt?: string // AI生成的图标描述（用于后续图标生成）
  theme_icon?: string // 备选：预定义的React图标名称
}

interface CrossPlatformReport {
  id?: number // 报告ID
  platform_reports: PlatformReport[]
  综合分析?: ComprehensiveAnalysis | null
  created_at: string
}

// 🚀 性能优化：平台配置常量（已在组件外部，避免重复创建）
const PLATFORMS = [
  {
    id: 'bilibili',
    name: 'Bilibili',
    icon: <SiBilibili />,
    color: 'from-blue-400 to-cyan-500',
    bg: 'bg-blue-50/10 dark:bg-blue-900/10',
    text: 'text-[#00A1D6]',
    border: 'border-blue-200/20 dark:border-blue-800/20',
    widgetType: 'bilibili',
  },
  {
    id: 'steam',
    name: 'Steam',
    icon: <FaSteam />,
    color: 'from-gray-700 to-gray-800',
    bg: 'bg-gray-50/10 dark:bg-neutral-900/10',
    text: 'text-gray-700 dark:text-gray-300',
    border: 'border-gray-200/20 dark:border-neutral-700/20',
    widgetType: 'gauge',
  },
  {
    id: 'github',
    name: 'GitHub',
    icon: <FaGithub />,
    color: 'from-gray-700 to-gray-900',
    bg: 'bg-gray-50/10 dark:bg-neutral-900/10',
    text: 'text-gray-600 dark:text-gray-400',
    border: 'border-gray-200/20 dark:border-neutral-700/20',
    widgetType: 'terminal',
  },
  {
    id: 'netease',
    name: '网易云',
    icon: <SiNeteasecloudmusic />,
    color: 'from-red-500 to-red-600',
    bg: 'bg-red-50/10 dark:bg-red-900/10',
    text: 'text-red-500',
    border: 'border-red-200/20 dark:border-red-800/20',
    widgetType: 'music',
  },
]

// 🔧 工具函数：处理B站图片URL，使用后端代理
function getBilibiliProxyUrl(cover?: string, title?: string): string {
  if (!cover) {
    return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || 'B')}&size=400&background=00A1D6&color=fff`
  }
  // 如果已经是代理URL（以/api/proxy开头），直接使用（相对路径）
  if (cover.startsWith('/api/proxy/')) {
    return cover
  }
  // 如果是完整的B站图片URL，需要通过后端代理
  if (cover.includes('hdslb.com') || cover.includes('bilibili.com')) {
    return `${API_URL || ''}/api/proxy/image?url=${encodeURIComponent(cover)}`
  }
  return cover
}

// 迷你组件：B站弹幕云 (优化：使用 memo + 优化动画性能 + 调度器)
const DanmakuWidget = memo(({ data, defaultDanmaku, triggerKey }: { data?: { danmaku?: string[] }, defaultDanmaku: string[], triggerKey?: unknown }) => {
  const texts = useMemo(() => data?.danmaku || defaultDanmaku, [data?.danmaku, defaultDanmaku])
  const anim = useAnimationLevel()
  const uniqueId = useId()

  // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
  const { isAnimating } = useLoopAnimation({
    duration: 11000, // 弹幕滚动约8秒 + 额外保持3秒
    trigger: triggerKey, // 状态切换时触发
    enabled: anim.loop, // 低端设备禁用
  })

  // 🆕 低性能模式：限制弹幕数量不超过3条
  const maxDanmakuCount = anim.loop ? (Math.random() < 0.7 ? (Math.random() < 0.5 ? 3 : 4) : 5) : 3

  // 🚀 性能优化：预计算随机化的动画参数，避免弹幕重叠
  const animations = useMemo(() => {
    const lanes = 5 // 总轨道数量

    // 🚀 优化：使用 Fisher-Yates 洗牌算法随机分配轨道，避免 do-while 循环
    const availableLanes = Array.from({ length: lanes }, (_, i) => i)
    for (let i = availableLanes.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [availableLanes[i], availableLanes[j]] = [availableLanes[j], availableLanes[i]]
    }

    return texts.slice(0, maxDanmakuCount).map((_, i) => ({
      duration: 6 + Math.random() * 4, // 6-10秒随机
      delay: i * 0.7 + Math.random() * 0.5, // 错开延迟，避免同时出现
      top: `${10 + availableLanes[i] * 18}%`, // 固定轨道位置，每条间隔18%
      opacity: 0.4 + Math.random() * 0.3, // 40%-70% 随机透明度
    }))
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
            repeat: 0, // 只运行一轮，由调度器 cooldown 控制重新播放
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

// 新组件：B站综合展示组件 (弹幕 + 资料库内容切换)
const BilibiliWidget = memo(({ data, onContentChange, showOverview, defaultDanmaku }: {
  data?: { danmaku?: string[], library_items?: Array<{ title: string, cover?: string, type: string }> }
  onContentChange?: (item: { title: string, type: string } | null) => void
  showOverview: boolean
  defaultDanmaku: string[]
}) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(libraryItems, showOverview)

  // 通知父组件当前内容变化
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
      {showOverview || !currentItem || libraryItems.length === 0 ? (
        <motion.div
          key="danmaku"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <DanmakuWidget data={data} defaultDanmaku={defaultDanmaku} triggerKey={showOverview} />
        </motion.div>
      ) : (
        <motion.div
          key={`library-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
            {/* 封面图片背景 */}
            <div className="absolute inset-0">
              <img
                src={getBilibiliProxyUrl(currentItem.cover, currentItem.title)}
                alt={currentItem.title}
                className="w-full h-full object-cover"
                loading="lazy"
              />
              {/* 半透明遮罩 */}
              <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

// 迷你组件：Steam统计展示
const SteamStatsWidget = memo(({ data, defaultPlayerType }: { data?: {
  hardcore_score?: number
  player_type?: string
  games_count?: number
  total_playtime?: number
}; defaultPlayerType: string }) => {
  const score = useMemo(() => data?.hardcore_score || 0, [data?.hardcore_score])
  const type = useMemo(() => data?.player_type || defaultPlayerType, [data?.player_type, defaultPlayerType])
  const gamesCount = useMemo(() => data?.games_count || 0, [data?.games_count])
  const totalPlaytime = useMemo(() => {
    const hours = data?.total_playtime || 0
    if (hours >= 1000)
      return `${(hours / 1000).toFixed(1)}k`
    return hours.toString()
  }, [data?.total_playtime])

  const { t } = useI18n()

  return (
    <div className="relative h-full w-full overflow-hidden">
      {/* 背景：对角分割设计 */}
      <div className="absolute inset-0">
        <div className="absolute inset-0 bg-gradient-to-br from-gray-100/50 to-transparent dark:from-white/[0.02] dark:to-transparent clip-diagonal" />
      </div>

      {/* 左侧：巨大评分数字 + 标签 */}
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

        {/* 玩家类型标签 */}
        <motion.div
          className="mt-2"
          initial={{ y: 10, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          transition={{ duration: 0.5, delay: 0.5 }}
        >
          <div className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-gray-800/90 dark:bg-white/90 backdrop-blur-sm">
            <div className="w-1.5 h-1.5 rounded-full bg-gray-300 dark:bg-black/60 animate-pulse" />
            <span className="text-[10px] font-bold text-gray-100 dark:text-gray-900 uppercase tracking-wide">{type}</span>
          </div>
        </motion.div>
      </motion.div>

      {/* 右侧：垂直统计条 */}
      <div className="absolute right-0 top-0 bottom-0 w-1/3 flex flex-col justify-center items-end pr-5 gap-4">
        {/* 游戏数量 */}
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

        {/* 游玩时长 */}
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

// 新组件：Steam综合展示组件 (统计 + 资料库内容切换)
const SteamWidget = memo(({ data, onContentChange, showOverview, defaultPlayerType }: {
  data?: {
    hardcore_score?: number
    player_type?: string
    games_count?: number
    total_playtime?: number
    library_items?: Array<{ title: string, cover?: string, type: string }>
  }
  onContentChange?: (item: { title: string, type: string } | null) => void
  showOverview: boolean
  defaultPlayerType: string
}) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(libraryItems, showOverview)

  // 通知父组件当前内容变化
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
      {showOverview || !currentItem || libraryItems.length === 0 ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <SteamStatsWidget data={data} defaultPlayerType={defaultPlayerType} />
        </motion.div>
      ) : (
        <motion.div
          key={`library-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
            {/* 封面图片背景 */}
            <div className="absolute inset-0">
              <img
                src={currentItem.cover || `https://ui-avatars.com/api/?name=${encodeURIComponent(currentItem.title)}&size=400&background=1b2838&color=fff`}
                alt={currentItem.title}
                className="w-full h-full object-cover"
                loading="lazy"
              />
              {/* 半透明遮罩 */}
              <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

// 迷你组件：GitHub综合统计展示（热力图 + 语言统计的融合设计）
const GithubStatsWidget = memo(({ data, defaultLevel, levelKeywords }: { data?: {
  contribution_level?: string
  total_contributions?: number
  repos_count?: number
  languages?: { name: string, percentage: number }[]
  contribution_calendar?: Array<{ date: string, count: number }>
}; defaultLevel: string; levelKeywords: { legendary: string, core: string, senior: string, prolific: string, active: string } }) => {
  const level = useMemo(() => data?.contribution_level || defaultLevel, [data?.contribution_level, defaultLevel])
  const contributions = useMemo(() => data?.total_contributions || 0, [data?.total_contributions])
  const reposCount = useMemo(() => data?.repos_count || 0, [data?.repos_count])
  const langs = useMemo(() => data?.languages || [], [data?.languages])
  const contributionCalendar = useMemo(() => {
    return data?.contribution_calendar || []
  }, [data?.contribution_calendar])

  const { t } = useI18n()

  // 根据contribution_level设置颜色
  const getLevelColor = (level: string) => {
    if (level.includes(levelKeywords.legendary) || level.includes(levelKeywords.core))
      return '#22c55e'
    if (level.includes(levelKeywords.senior) || level.includes(levelKeywords.prolific))
      return '#3b82f6'
    if (level.includes(levelKeywords.active))
      return '#8b5cf6'
    return '#6b7280'
  }

  const levelColor = getLevelColor(level)

  // 从真实贡献数据生成热力图网格 (按周-天的布局)
  const generateHeatmapGrid = () => {
    const grid = []

    if (contributionCalendar.length > 0) {
      // 使用真实数据 - 取最近60天 (12周 x 5工作日)
      const recentDays = contributionCalendar.slice(-60)
      const maxCount = Math.max(...recentDays.map(d => d.count), 1)

      // 按周-天的方式排列数据
      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const index = week * 5 + day
          const dayData = recentDays[index]
          const count = dayData?.count || 0

          // 根据实际提交次数计算透明度
          const opacity = count > 0
            ? Math.min((count / maxCount) * 0.85 + 0.15, 1)
            : 0.12

          grid.push({
            week,
            day,
            opacity,
            count,
          })
        }
      }
    }
    else {
      // 后备方案：基于总贡献数生成模拟数据
      const avgPerDay = contributions / 365

      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          // 使用泊松分布模拟更真实的贡献模式
          const lambda = avgPerDay * (0.5 + Math.random())
          const count = Math.floor(-Math.log(1 - Math.random()) * lambda)
          const opacity = count > 0
            ? Math.min(count / (avgPerDay * 2) * 0.7 + 0.15, 1)
            : 0.12

          grid.push({
            week,
            day,
            opacity,
            count,
          })
        }
      }
    }

    return grid
  }

  const heatmapData = useMemo(() => generateHeatmapGrid(), [contributionCalendar, contributions])

  // 获取语言对应的颜色
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
      {/* 背景渐变 */}
      <div className="absolute inset-0 bg-gradient-to-br from-gray-50/50 to-transparent dark:from-white/[0.02] dark:to-transparent" />

      {/* 主要内容区域 - 垂直布局 */}
      <div className="relative h-full flex flex-col p-2 justify-between">
        {/* 顶部区域：等级标签 + 热力图 + 数据统计 */}
        <div className="space-y-2">
          {/* 等级徽章和热力图 */}
          <div className="flex items-start justify-between">
            {/* 左：等级徽章 + 数据统计 */}
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

              {/* 关键数据（在标签下方，垂直排列） */}
              <div className="flex flex-col gap-1.5">
                {/* 提交数 */}
                <motion.div
                  className="flex items-baseline gap-1.5"
                  initial={{ y: 10, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.3 }}
                >
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-200 leading-none">{contributions}</span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">{t.reportsPage.commits}</span>
                </motion.div>

                {/* 仓库数 */}
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

            {/* 右：热力图网格 - 增大显示 */}
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

        {/* 底部：语言统计条（避开左下角Logo，从中间位置开始） */}
        <div className="space-y-1 pb-1">
          {langs.slice(0, 2).map((lang, i) => (
            <motion.div
              key={i}
              className="relative pl-12"
              initial={{ x: -20, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.5 + i * 0.1 }}
            >
              {/* 语言名称 + 百分比 */}
              <div className="flex items-center justify-between mb-0.5">
                <span className="text-[8px] font-bold text-gray-700 dark:text-gray-300">{lang.name}</span>
                <span className="text-[7px] font-mono text-gray-500 dark:text-gray-400">
                  {lang.percentage}
                  %
                </span>
              </div>
              {/* 进度条 */}
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
          {langs.length === 0 && (
            <div className="text-[8px] font-mono text-gray-400 text-center opacity-50">
              {t.reportsPage.analyzingRepos}
            </div>
          )}
        </div>
      </div>
    </div>
  )
})

// 新组件：GitHub综合展示组件 (统计信息 + 项目卡片双态循环)
const GithubWidget = memo(({ data, onContentChange, showOverview, defaultLevel, levelKeywords }: {
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
  defaultLevel: string
  levelKeywords: { legendary: string, core: string, senior: string, prolific: string, active: string }
}) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data?.library_items])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(libraryItems, showOverview)

  // 通知父组件当前内容变化
  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: currentItem.language || 'repo' })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  const { t } = useI18n()

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
          <GithubStatsWidget data={data} defaultLevel={defaultLevel} levelKeywords={levelKeywords} />
        </motion.div>
      ) : (
        <motion.div
          key={`library-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          {currentItem ? (
            <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
              {/* 仓库信息展示 */}
              <div className="absolute inset-0 bg-gradient-to-br from-gray-800 to-gray-900 dark:from-black dark:to-black/90">
                <div className="absolute inset-0 flex flex-col p-2.5 pb-[20%]">
                  {/* 顶部：Stars和Forks统计 */}
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

                  {/* 项目描述 */}
                  {currentItem.description && (
                    <div className="text-[10px] leading-snug text-gray-200 line-clamp-4 px-1">
                      {currentItem.description}
                    </div>
                  )}
                </div>
              </div>
            </div>
          ) : (
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

// 迷你组件：网易云音乐卡片展示（统计信息展示）+ 调度器
const MusicStatsWidget = memo(({ data, tenThousandSuffix, triggerKey }: { data?: {
  soul_color?: string
  mood_keywords?: Array<{ tag: string, color: string }>
  follower_count?: number
  playlist_count?: number
  level?: number
}; tenThousandSuffix: string; triggerKey?: unknown }) => {
  const anim = useAnimationLevel()
  const uniqueId = useId()

  // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
  const { isAnimating } = useLoopAnimation({
    duration: 5000, // 气泡动画约5秒
    trigger: triggerKey, // 状态切换时触发
    enabled: anim.loop, // 低端设备禁用
  })

  const canAnimate = anim.loop && isAnimating
  const { t } = useI18n()

  const color = useMemo(() => data?.soul_color || '#ef4444', [data?.soul_color])
  const moodKeywords = useMemo(() => data?.mood_keywords || [], [data?.mood_keywords])
  const followerCount = useMemo(() => data?.follower_count || 0, [data?.follower_count])
  const playlistCount = useMemo(() => data?.playlist_count || 0, [data?.playlist_count])
  const level = useMemo(() => data?.level || 0, [data?.level])

  // 格式化大数字
  const formatNumber = (num: number) => {
    if (num >= 10000)
      return `${(num / 10000).toFixed(1)}${tenThousandSuffix}`
    if (num >= 1000)
      return `${(num / 1000).toFixed(1)}k`
    return num.toString()
  }

  // 生成气泡位置 - 使用 Mitchell's Best-Candidate 算法确保随机分布且不重叠
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

    // 定义统计数据区域（右下角），避免气泡遮挡
    // 假设统计区域大约占据右侧 35% 和底部 35%
    const isOverlappingStats = (x: number, y: number) => {
      return x > 60 && y > 60
    }

    moodKeywords.forEach((keyword, i) => {
      // 增大随机性：35px - 95px
      const size = 35 + Math.floor(hash(keyword.tag, 1) * 60)

      let bestX = 50
      let bestY = 50
      let maxMinDist = -1

      // 为每个气泡尝试 30 个候选位置，选择离其他气泡最远的一个
      for (let attempt = 0; attempt < 30; attempt++) {
        // 使用哈希保证确定性随机
        const r1 = hash(keyword.tag, 100 + attempt + i * 50)
        const r2 = hash(keyword.tag, 200 + attempt + i * 50)

        // 生成位置 (留出 10% 边距)
        const x = 10 + r1 * 80
        const y = 10 + r2 * 80

        // 避开右下角统计区域
        if (isOverlappingStats(x, y))
          continue

        // 计算到最近邻居的距离
        let minDist = 1000
        if (items.length === 0) {
          minDist = 1000
        }
        else {
          for (const item of items) {
            // 简单的欧几里得距离（百分比空间）
            // 考虑到卡片大概是 2:1 的比例，Y 轴的百分比距离权重加倍
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
        floatDuration: 3 + hash(keyword.tag, 4) * 4, // 3-7s
        floatDelay: hash(keyword.tag, 5) * 2, // 0-2s
      })
    })

    return items
  }, [moodKeywords])

  return (
    <div className="relative h-full w-full overflow-hidden">
      {/* 背景渐变 */}
      <div className="absolute inset-0 bg-gradient-to-br from-red-50/50 to-transparent dark:from-red-900/20 dark:to-transparent" />

      {/* 主要内容区域 */}
      <div className="relative h-full w-full p-3">
        {/* 风格标签动态气泡云 - 绝对定位分布 */}
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
                marginLeft: `-${bubble.size / 2}px`, // 居中定位
                marginTop: `-${bubble.size / 2}px`,
                // 气泡质感优化：径向渐变 + 内部高光 + 柔和阴影
                background: `radial-gradient(120% 120% at 30% 30%, rgba(255,255,255,0.6) 0%, ${bubble.color}20 20%, ${bubble.color}60 100%)`,
                border: `1px solid rgba(255,255,255,0.3)`, // 极细的白色半透明边框
                color: bubble.color,
                // 动态字体大小，限制最小和最大值
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
              {/* 高光点缀 */}
              <div className="absolute top-[15%] left-[15%] w-[20%] h-[10%] bg-white/30 rounded-full blur-[1px] transform -rotate-45" />
              <span className="relative z-10 mix-blend-multiply dark:mix-blend-normal">{bubble.tag}</span>
            </motion.div>
          ))}
        </div>

        {/* 右下角：统计数据 */}
        <div className="absolute bottom-3 right-3 flex flex-col items-end gap-2 z-20">
          {/* 等级标签 */}
          <motion.div
            className="px-2.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1 backdrop-blur-md shadow-lg bg-gradient-to-br from-red-50 to-red-100 dark:from-red-950/80 dark:to-red-900/60 text-red-600 dark:text-red-300"
            style={{
              boxShadow: '0 2px 12px rgba(239, 68, 68, 0.25)',
            }}
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

          {/* 粉丝和歌单统计 */}
          <motion.div
            className="flex items-center gap-2 px-3 py-1.5 rounded-xl backdrop-blur-md shadow-lg bg-white/90 dark:bg-black/90 border border-white/30 dark:border-white/10"
            style={{
              backdropFilter: 'blur(10px)',
            }}
            initial={{ scale: 0.8, opacity: 0, x: 20 }}
            animate={{ scale: 1, opacity: 1, x: 0 }}
            transition={{ duration: 0.4, delay: 0.2 }}
          >
            <div className="flex flex-col items-end">
              <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                {formatNumber(followerCount)}
              </span>
              <div className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">
                {t.reportsPage.fans}
              </div>
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

// 新组件：网易云综合展示组件 (音乐统计 + 两张音乐卡片同时显示)
const NeteaseWidget = memo(({
  data,
  onContentChange,
  showOverview,
  tenThousandSuffix,
}: {
  data?: any // 使用 any 兼容后端多种数据格式
  onContentChange?: (item: { titles: string[], type: string } | null) => void
  showOverview: boolean
  tenThousandSuffix: string
}) => {
  // 数据转换和类型安全处理
  const processedData = useMemo(() => {
    if (!data)
      return undefined

    // 转换 mood_keywords 格式（兼容旧格式）
    let moodKeywords: Array<{ tag: string, color: string }> = []
    if (data.mood_keywords) {
      if (Array.isArray(data.mood_keywords)) {
        if (data.mood_keywords.length > 0) {
          if (typeof data.mood_keywords[0] === 'string') {
            // 旧格式：字符串数组，需要转换
            const defaultColors = ['#7B68EE', '#FF6B9D', '#4ECDC4', '#FFB347', '#95E1D3']
            moodKeywords = data.mood_keywords.map((tag: string, i: number) => ({
              tag,
              color: defaultColors[i % defaultColors.length],
            }))
          }
          else if (typeof data.mood_keywords[0] === 'object') {
            // 新格式：对象数组
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

  // 当切换回概览模式时，更新下一个要显示的项目的索引（一次跳过2个，因为显示2首歌）
  useEffect(() => {
    if (showOverview && libraryItems.length > 0) {
      setCurrentItemIndex(prev => (prev + 2) % libraryItems.length)
    }
  }, [showOverview, libraryItems.length])

  const currentItems = useMemo(() => [
    libraryItems[currentItemIndex],
    libraryItems[(currentItemIndex + 1) % libraryItems.length],
  ].filter(Boolean), [libraryItems, currentItemIndex])

  // 通知父组件当前内容变化
  useEffect(() => {
    if (!showOverview && currentItems.length > 0) {
      onContentChange?.({
        titles: currentItems.map(item => item.title),
        type: 'music',
      })
    }
    else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItems, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || currentItems.length === 0 || libraryItems.length === 0 ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <MusicStatsWidget data={processedData} tenThousandSuffix={tenThousandSuffix} triggerKey={showOverview} />
        </motion.div>
      ) : (
        <motion.div
          key={`music-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          {/* 两张音乐卡片并排显示 */}
          <div className="h-full w-full flex gap-1.5">
            {currentItems.map((item, idx) => (
              <div key={idx} className="flex-1 h-full">
                <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                  {/* 封面图片背景 */}
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

// 🚀 性能优化：将综合报告卡片提取为独立的 memo 组件
export default function Reports() {
  // 🆕 初始化报告页调度器（Visibility + Interval + RAF + DOMBatch）
  useReportsScheduler()

  const { t } = useI18n()
  const isPageReady = usePageReady()
  // 🆕 标题字体 Hook
  const { currentFont, titleFontSize, titleColor } = useTitleFont()

  // 二级导航项配置
  const navItems: SecondaryNavItem[] = useMemo(() => [
    {
      id: 'platform',
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0a4 4 0 004-4v-4a2 2 0 012-2h4a2 2 0 012 2v4a4 4 0 01-4 4h-8z" />
        </svg>
      ),
      label: t.nav.platformReport,
      title: t.nav.platformReport,
      ariaLabel: t.nav.showPlatformReport,
    },
    {
      id: 'comprehensive',
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v13m0-13V6a2 2 0 112 2h-2zm0 0V5.5A2.5 2.5 0 109.5 8H12zm-7 4h14M5 12a2 2 0 110-4h14a2 2 0 110 4M5 12v7a2 2 0 002 2h10a2 2 0 002-2v-7" />
        </svg>
      ),
      label: t.nav.comprehensiveReport,
      title: t.nav.comprehensiveReport,
      ariaLabel: t.nav.showComprehensiveReport,
    },
  ], [t])

  // 使用二级导航 Hook
  const { activeId: activeTab, setActiveId: setActiveTab, setExpanded } = useSecondaryNav({
    routePath: '/reports',
    items: navItems,
    defaultActiveId: 'platform',
    expandHint: t.nav.switchTab,
  })

  // 监听展开事件
  useEffect(() => {
    const handleExpandSecondary = (e: CustomEvent<{ path: string }>) => {
      if (e.detail.path === '/reports') {
        setExpanded(true)
      }
    }

    window.addEventListener('nav-expand-secondary', handleExpandSecondary as EventListener)
    return () => {
      window.removeEventListener('nav-expand-secondary', handleExpandSecondary as EventListener)
    }
  }, [setExpanded])

  // 检测深色模式
  const [isDark, setIsDark] = useState(false)
  useEffect(() => {
    const checkDarkMode = () => {
      setIsDark(document.documentElement.classList.contains('dark'))
    }
    checkDarkMode()
    const observer = new MutationObserver(checkDarkMode)
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    return () => observer.disconnect()
  }, [])

  // 计算标题颜色
  const getTitleColor = (colorType: 'primary' | 'accent' = 'primary') => {
    // 如果设置了自定义颜色，使用自定义颜色
    if (titleColor !== 'primary') {
      if (titleColor === 'adaptive') {
        return isDark
          ? 'color-mix(in srgb, var(--color-primary) 50%, #ffffff)'
          : 'color-mix(in srgb, var(--color-primary) 50%, #000000)'
      }
      const colorMap: Record<string, string> = {
        secondary: 'var(--color-secondary)',
        accent: 'var(--color-accent)',
        light: 'var(--color-light)',
        dark: 'var(--color-dark)',
      }
      return `color-mix(in srgb, ${colorMap[titleColor] || 'var(--color-primary)'} 70%, transparent)`
    }
    // 否则使用默认颜色（primary 或 accent）
    return colorType === 'accent'
      ? 'color-mix(in srgb, var(--color-accent) 70%, transparent)'
      : 'color-mix(in srgb, var(--color-primary) 70%, transparent)'
  }

  const [loadingPlatform, setLoadingPlatform] = useState<string | null>(null)
  const [report, setReport] = useState<CrossPlatformReport | null>(null)
  const [selectedPlatform, setSelectedPlatform] = useState<string | null>(null)
  const [isPlaying, setIsPlaying] = useState(true)
  const [customStyle, setCustomStyle] = useState<string>('')
  // 🚀 性能优化：防抖处理用户输入
  const debouncedCustomStyle = useDebounce(customStyle, 300)
  // 🆕 动画级别控制，用于装饰性动画
  const anim = useAnimationLevel()
  const [isAdmin, setIsAdmin] = useState(false)
  const [isComprehensiveExpanded, setIsComprehensiveExpanded] = useState(false)
  const [comprehensiveReports, setComprehensiveReports] = useState<any[]>([]) // 所有综合报告列表
  const [currentComprehensiveId, setCurrentComprehensiveId] = useState<number | null>(null) // 当前打开的综合报告ID
  const [isInputExpanded, setIsInputExpanded] = useState(false) // 输入框展开状态

  // 🎭 舞台模式状态
  const [isStageMode, setIsStageMode] = useState(false)
  const [stagePaused, setStagePaused] = useState(false) // 舞台模式暂停状态
  const [refreshingStage, setRefreshingStage] = useState(false) // 刷新舞台报告加载状态
  const [toastMessage, setToastMessage] = useState<string>('') // Toast消息
  const [playAllMode, setPlayAllMode] = useState(false) // 播放所有模式
  const [playAllQueue, setPlayAllQueue] = useState<string[]>([]) // 播放队列
  const playAllQueueRef = useRef<string[]>([]) // 用ref保存队列，避免闭包问题
  const [stageReportData, setStageReportData] = useState<{
    platform?: string
    summary?: string
    insights?: string[]
    card_visuals?: any
    // 综合报告字段
    综合分析?: any
    library_items?: Array<{ title: string, cover?: string, type: string, platform?: string }>
    type?: 'platform' | 'comprehensive'
    title?: string // 综合报告标题
  } | null>(null)

  // 新增：全局卡片状态切换控制
  const [showOverview, setShowOverview] = useState(true)

  // i18n: 翻译后的平台配置
  const translatedPlatforms = useMemo(() => PLATFORMS.map(p => ({
    ...p,
    name: p.id === 'netease'
      ? t.reportsPage.neteaseMusic
      : p.id === 'bilibili'
        ? t.reportsPage.bilibili
        : p.id === 'steam'
          ? t.reportsPage.steam
          : p.id === 'github' ? t.reportsPage.github : p.name,
  })), [t.reportsPage.neteaseMusic, t.reportsPage.bilibili, t.reportsPage.steam, t.reportsPage.github])

  // i18n: 默认弹幕文本
  const defaultDanmaku = useMemo(() => t.reportsPage.danmakuDefaults, [t.reportsPage.danmakuDefaults])

  // i18n: 默认玩家类型
  const defaultPlayerType = useMemo(() => t.reportsPage.casualPlayer, [t.reportsPage.casualPlayer])

  // i18n: 默认开发者级别
  const defaultDevLevel = useMemo(() => t.reportsPage.activeDeveloper, [t.reportsPage.activeDeveloper])

  // i18n: 级别关键词（用于颜色匹配）
  const levelKeywords = useMemo(() => ({
    legendary: t.reportsPage.legendary,
    core: t.reportsPage.core,
    senior: t.reportsPage.senior,
    prolific: t.reportsPage.prolific,
    active: t.reportsPage.active,
  }), [t.reportsPage.legendary, t.reportsPage.core, t.reportsPage.senior, t.reportsPage.prolific, t.reportsPage.active])

  // 🆕 使用可见性感知定时器 - 页面隐藏时自动暂停轮播
  // 当舞台模式开启时，停止卡片轮播并保持在概览状态
  useReportsVisibilityInterval(() => {
    if (!isStageMode) {
      setShowOverview(prev => !prev)
    }
  }, isStageMode ? null : 10000)

  // 舞台模式开启时重置为概览状态
  useEffect(() => {
    if (isStageMode) {
      setShowOverview(true)
    }
  }, [isStageMode])

  // 🚀 修复：将卡片内容状态提升到父组件，避免在循环中使用 useState
  const [cardContents, setCardContents] = useState<Record<string, any>>({})

  const handleContentChange = useCallback((platformId: string, content: any) => {
    setCardContents((prev) => {
      // 性能优化：使用浅比较替代 JSON.stringify
      // 如果是同一引用或 null/undefined 相同，不更新状态
      const prevContent = prev[platformId]
      if (prevContent === content)
        return prev
      // 如果都是对象，检查关键字段（避免深比较）
      if (prevContent && content && typeof prevContent === 'object' && typeof content === 'object') {
        // 检查 id 或 timestamp 等标识字段
        if (prevContent.id === content.id && prevContent.timestamp === content.timestamp) {
          return prev
        }
      }
      return { ...prev, [platformId]: content }
    })
  }, [])

  // 🚀 性能优化：缓存所有平台的内容变更处理函数，避免传递内联函数导致子组件重复渲染
  const contentChangeHandlers = useMemo(() => {
    const handlers: Record<string, (content: any) => void> = {}
    PLATFORMS.forEach((p) => {
      handlers[p.id] = (content: any) => handleContentChange(p.id, content)
    })
    return handlers
  }, [handleContentChange])

  // 🚀 性能优化：监听舞台暂停状态
  useEffect(() => {
    const handlePauseStateChange = (e: CustomEvent<{ isPaused: boolean }>) => {
      setStagePaused(e.detail.isPaused)
    }

    window.addEventListener('stage-pause-state-change', handlePauseStateChange as EventListener)

    return () => {
      window.removeEventListener('stage-pause-state-change', handlePauseStateChange as EventListener)
    }
  }, [])

  // 🚀 性能优化：缓存平台报告映射，避免重复查找
  const platformReportsMap = useMemo(() => {
    const map = new Map<string, PlatformReport>()
    report?.platform_reports?.forEach(r => map.set(r.platform, r))
    return map
  }, [report?.platform_reports])

  // 🚀 性能优化：缓存显示的综合报告列表（限制20个）
  const displayedComprehensiveReports = useMemo(() =>
    comprehensiveReports.slice(0, 20), [comprehensiveReports])

  // 🎭 打开舞台模式
  const openStageMode = useCallback((platformId: string) => {
    const platformReport = platformReportsMap.get(platformId)
    if (platformReport) {
      setStageReportData({
        type: 'platform',
        platform: platformId,
        summary: platformReport.summary,
        insights: platformReport.insights,
        card_visuals: platformReport.card_visuals,
      })
      setIsStageMode(true)
    }
  }, [platformReportsMap])

  // 🎭 关闭舞台模式
  const closeStageMode = useCallback(() => {
    setIsStageMode(false)
    setPlayAllMode(false)
    setPlayAllQueue([])
    playAllQueueRef.current = []
    // 不立即清除数据，以便播放退出动画
    // setStageReportData(null);
  }, [])

  // 🎭 用户手动关闭舞台（无论什么模式都完全退出）
  const handleUserCloseStage = useCallback(() => {
    closeStageMode()
  }, [closeStageMode])

  // 🎭 开始播放所有平台
  const startPlayAll = useCallback(() => {
    // 获取所有有报告的平台
    const platformsWithReports = PLATFORMS
      .filter(p => platformReportsMap.has(p.id))
      .map(p => p.id)

    if (platformsWithReports.length === 0) {
      setToastMessage(`✗ ${t.reportsPage.noPlatformReports}`)
      return
    }

    // 播放第一个平台，将剩余平台放入队列
    const firstPlatformId = platformsWithReports[0]
    const remainingPlatforms = platformsWithReports.slice(1)

    playAllQueueRef.current = remainingPlatforms
    setPlayAllQueue(remainingPlatforms)
    setPlayAllMode(true)
    setIsStageMode(true)

    const platformReport = platformReportsMap.get(firstPlatformId)
    if (platformReport) {
      setStageReportData({
        type: 'platform',
        platform: firstPlatformId,
        summary: platformReport.summary,
        insights: platformReport.insights,
        card_visuals: platformReport.card_visuals,
      })
    }
  }, [platformReportsMap])

  // 🎭 播放下一个平台（播放所有模式）
  const playNextPlatform = useCallback(() => {
    if (playAllQueueRef.current.length === 0) {
      // 所有平台播放完毕，退出舞台模式
      closeStageMode()
      setToastMessage(`✓ ${t.reportsPage.allPlaybackComplete}`)
      return
    }

    // 取出下一个平台并更新队列
    const nextPlatformId = playAllQueueRef.current[0]
    const remainingQueue = playAllQueueRef.current.slice(1)

    playAllQueueRef.current = remainingQueue
    setPlayAllQueue(remainingQueue)

    // 不关闭舞台，只更新reportData，StageMode会自动重置章节
    const platformReport = platformReportsMap.get(nextPlatformId)
    if (platformReport) {
      setStageReportData({
        type: 'platform',
        platform: nextPlatformId,
        summary: platformReport.summary,
        insights: platformReport.insights,
        card_visuals: platformReport.card_visuals,
      })
    }
  }, [platformReportsMap, closeStageMode])

  // 监听StageMode结束事件，在播放所有模式下自动播放下一个
  useEffect(() => {
    const handleStageComplete = () => {
      if (playAllMode && isStageMode) {
        // 等待一小段时间再播放下一个
        setTimeout(() => {
          playNextPlatform()
        }, 500)
      }
    }

    window.addEventListener('stage-playback-complete', handleStageComplete)
    return () => {
      window.removeEventListener('stage-playback-complete', handleStageComplete)
    }
  }, [playAllMode, isStageMode, playNextPlatform])

  // 🎭 刷新当前舞台模式的平台报告
  const refreshStageReport = useCallback(async () => {
    if (!stageReportData?.platform || stageReportData.type !== 'platform') {
      return
    }

    const platformId = stageReportData.platform
    const platformName = translatedPlatforms.find(p => p.id === platformId)?.name || platformId

    setRefreshingStage(true)

    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        setToastMessage(`✗ ${t.reportsPage.getTokenFailed}`)
        setRefreshingStage(false)
        return
      }

      setToastMessage(`✓ ${t.reportsPage.refreshingReport.replace('{platform}', platformName)}`)

      // 1. 刷新该平台的数据
      try {
        await fetch(`${API_URL}/api/profile/fetch-platform`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': csrfToken,
          },
          credentials: 'include',
          body: JSON.stringify({ platform: platformId }),
        })
      }
      catch (fetchErr) {
        console.warn(`Refresh ${platformId} data request error:`, fetchErr)
      }

      // 2. 生成新报告
      const response = await fetch(`${API_URL}/api/reports/platform`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ platforms: [platformId] }),
      })

      if (!response.ok)
        throw new Error(t.reportsPage.generateFailed)

      // 3. 获取最新的平台报告
      const latestResponse = await fetch(`${API_URL}/api/reports/latest`, {
        credentials: 'include',
      })

      if (latestResponse.ok) {
        const latestData = await latestResponse.json()
        if (latestData.platform_reports) {
          // 更新报告数据
          setReport(prev => ({
            platform_reports: latestData.platform_reports,
            综合分析: prev?.综合分析 || null,
            created_at: latestData.created_at || new Date().toISOString(),
          }))

          // 更新舞台模式显示的数据
          const updatedPlatformReport = latestData.platform_reports.find(
            (r: PlatformReport) => r.platform === platformId,
          )
          if (updatedPlatformReport) {
            setStageReportData({
              type: 'platform',
              platform: platformId,
              summary: updatedPlatformReport.summary,
              insights: updatedPlatformReport.insights,
              card_visuals: updatedPlatformReport.card_visuals,
            })
            setToastMessage(`✓ ${t.reportsPage.reportRefreshSuccess.replace('{platform}', platformName)}`)
          }
          else {
            setToastMessage(`✗ ${t.reportsPage.reportRefreshNoData}`)
          }
        }
        else {
          setToastMessage(`✗ ${t.reportsPage.getLatestReportFailed}`)
        }
      }
      else {
        setToastMessage(`✗ ${t.reportsPage.getLatestReportFailed}`)
      }
    }
    catch (err) {
      console.error('Refresh stage report failed:', err)
      setToastMessage(`✗ ${t.reportsPage.refreshReportFailed.replace('{platform}', platformName)}`)
    }
    finally {
      setRefreshingStage(false)
    }
  }, [stageReportData])

  // 使用 AuthContext 获取管理员状态
  const { isAdmin: authIsAdmin, isAuthenticated, hasChecked, checkAuth } = useAuth()

  // 智能检测：如果有登录迹象且未检查过，触发认证检查
  useEffect(() => {
    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }
  }, [hasChecked, checkAuth])

  useEffect(() => {
    setIsAdmin(authIsAdmin)
  }, [authIsAdmin, isAuthenticated])

  // 加载最新报告（平台报告 + 综合报告）- 性能优化版
  useEffect(() => {
    const fetchLatestReport = async () => {
      try {
        // 🚀 性能优化：并行获取平台报告和综合报告列表
        const [platformResponse, comprehensiveResponse] = await Promise.all([
          fetch(`${API_URL}/api/reports/latest`, {
            credentials: 'include',
          }),
          fetch(`${API_URL}/api/reports/comprehensive/list`, {
            credentials: 'include',
          }),
        ])

        let platformReports = []
        let createdAt = new Date().toISOString()

        // 1. 处理平台报告
        if (platformResponse.ok) {
          const platformData = await platformResponse.json()
          if (platformData.platform_reports) {
            platformReports = platformData.platform_reports
            createdAt = platformData.created_at || createdAt
          }
        }

        // 2. 处理综合报告列表
        if (comprehensiveResponse.ok) {
          const comprehensiveData = await comprehensiveResponse.json()
          if (comprehensiveData.success && comprehensiveData.reports && comprehensiveData.reports.length > 0) {
            // 🚀 性能优化：并行获取所有综合报告的详情（批量请求）
            const detailPromises = comprehensiveData.reports.map(async (report: any) => {
              try {
                const detailResponse = await fetch(`${API_URL}/api/reports/comprehensive/${report.id}`, {
                  credentials: 'include',
                })
                if (detailResponse.ok) {
                  const detailData = await detailResponse.json()
                  if (detailData.success && detailData.report) {
                    return {
                      ...report,
                      综合分析: detailData.report.综合分析 || null,
                    }
                  }
                }
              }
              catch (err) {
                console.error(`获取报告 ${report.id} 详情失败:`, err)
              }
              return report
            })

            const reportsWithDetails = await Promise.all(detailPromises)
            setComprehensiveReports(reportsWithDetails)
          }
        }

        // 3. 设置平台报告
        setReport({
          platform_reports: platformReports,
          综合分析: null, // 综合分析由comprehensiveReports单独管理
          created_at: createdAt,
        })
      }
      catch (err) {
        console.error('获取最新报告失败:', err)
      }
    }

    fetchLatestReport()
  }, [])

  // 生成单个平台报告 - 性能优化：使用 useCallback
  const generatePlatformReport = useCallback(async (platformId: string) => {
    setLoadingPlatform(platformId)
    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken)
        return

      // 1. 先刷新该平台的数据
      try {
        const fetchResponse = await fetch(`${API_URL}/api/profile/fetch-platform`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': csrfToken,
          },
          credentials: 'include',
          body: JSON.stringify({ platform: platformId }),
        })

        if (!fetchResponse.ok) {
          console.warn(`刷新 ${platformId} 数据失败，尝试使用现有数据生成报告`)
        }
      }
      catch (fetchErr) {
        console.warn(`刷新 ${platformId} 数据请求出错:`, fetchErr)
      }

      // 2. 生成报告
      const response = await fetch(`${API_URL}/api/reports/platform`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ platforms: [platformId] }),
      })

      if (!response.ok)
        throw new Error(t.reportsPage.generateFailed)

      // 🚀 性能优化：只获取最新的平台报告列表，不重复获取综合报告
      const latestResponse = await fetch(`${API_URL}/api/reports/latest`, {
        credentials: 'include',
      })

      if (latestResponse.ok) {
        const latestData = await latestResponse.json()
        if (latestData.platform_reports) {
          setReport(prev => ({
            platform_reports: latestData.platform_reports,
            综合分析: prev?.综合分析 || null,
            created_at: latestData.created_at || new Date().toISOString(),
          }))
        }
      }
    }
    catch (err) {
      console.error('Generate platform report failed:', err)
    }
    finally {
      setLoadingPlatform(null)
    }
  }, [t.reportsPage.generateFailed])

  // 生成综合分析 (基于已有平台报告) - 性能优化：使用 useCallback
  const generateComprehensiveReport = useCallback(async () => {
    setLoadingPlatform('comprehensive')
    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken)
        return

      // 构建请求体 - 只包含有值的字段
      const requestBody: any = {}
      if (debouncedCustomStyle && debouncedCustomStyle.trim()) {
        requestBody.style = debouncedCustomStyle.trim()
      }

      const response = await fetch(`${API_URL}/api/reports/comprehensive`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify(requestBody),
      })

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}))
        console.error('Generation failed:', errorData)
        throw new Error(errorData.message || t.reportsPage.generateFailed)
      }

      const data = await response.json()
      if (data.success && data.report) {
        // 🚀 性能优化：并行获取综合报告列表和详情
        const listResponse = await fetch(`${API_URL}/api/reports/comprehensive/list`, {
          credentials: 'include',
        })

        if (listResponse.ok) {
          const listData = await listResponse.json()
          if (listData.success && listData.reports) {
            // 并行获取所有综合报告的详情
            const detailPromises = listData.reports.map(async (report: any) => {
              try {
                const detailResponse = await fetch(`${API_URL}/api/reports/comprehensive/${report.id}`, {
                  credentials: 'include',
                })
                if (detailResponse.ok) {
                  const detailData = await detailResponse.json()
                  if (detailData.success && detailData.report) {
                    return {
                      ...report,
                      综合分析: detailData.report.综合分析 || null,
                    }
                  }
                }
              }
              catch (err) {
                console.error(`Get report ${report.id} details failed:`, err)
              }
              return report
            })

            const reportsWithDetails = await Promise.all(detailPromises)
            setComprehensiveReports(reportsWithDetails)
          }
        }
      }
      else if (!data.success && data.message) {
        console.warn(data.message)
        alert(data.message)
      }
    }
    catch (err) {
      console.error('Generate comprehensive report failed:', err)
      alert(err instanceof Error ? err.message : t.reportsPage.generateFailedRetry)
    }
    finally {
      setLoadingPlatform(null)
    }
  }, [debouncedCustomStyle, t.reportsPage.generateFailed, t.reportsPage.generateFailedRetry])

  // 🚀 性能优化：使用 useMemo 缓存计算结果
  const selectedReport = useMemo(() =>
    selectedPlatform && report
      ? report.platform_reports.find(r => r.platform === selectedPlatform)
      : null, [selectedPlatform, report])

  // 🚀 性能优化：缓存平台卡片点击处理器
  const handlePlatformClick = useCallback((platformId: string, hasReport: boolean) => {
    if (hasReport) {
      setSelectedPlatform(platformId)
    }
    else if (isAdmin) {
      generatePlatformReport(platformId)
    }
    else {
      setToastMessage(`⛗ ${t.reportsPage.adminOnlyGenerate}`)
    }
  }, [generatePlatformReport, isAdmin, t.reportsPage.adminOnlyGenerate])

  // 🚀 性能优化：缓存删除报告处理器
  const handleDeleteReport = useCallback(async (reportId: number) => {
    if (!window.confirm(t.reportsPage.confirmDeleteReport)) {
      return
    }

    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken)
        return

      const response = await fetch(`${API_URL}/api/reports/comprehensive/${reportId}`, {
        method: 'DELETE',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
      })

      const data = await response.json()
      if (data.success) {
        setIsComprehensiveExpanded(false)
        setCurrentComprehensiveId(null)
        setComprehensiveReports(prev => prev.filter(r => r.id !== reportId))
        setReport(prev => prev ? { ...prev, 综合分析: null } : null)
      }
      else {
        alert(data.message || t.reportsPage.deleteFailed)
      }
    }
    catch (err) {
      console.error('Delete report failed:', err)
      alert(t.reportsPage.deleteFailedRetry)
    }
  }, [t.reportsPage.deleteFailed, t.reportsPage.deleteFailedRetry, t.reportsPage.confirmDeleteReport])

  return (
    <AnimatedView className="h-screen overflow-hidden">
      {/* Toast提示 */}
      {toastMessage && <Toast message={toastMessage} onClose={() => setToastMessage('')} />}

      {/* 🎭 舞台模式组件 - 固定在顶部 */}
      <StageMode
        isOpen={isStageMode}
        onClose={handleUserCloseStage}
        reportData={stageReportData}
        onRefresh={refreshStageReport}
        playAllMode={playAllMode}
      />
      <div className="h-full flex flex-col pt-20 pb-24 md:pb-6 px-3 xs:px-4 sm:px-6">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col gap-4 p-2">
          {/* 上半部分：报告详情展示区域 (60%) - 保持占位但条件显示内容 */}
          <div className="h-[60%] rounded-2xl relative overflow-hidden">

            {/* 原有详情展示（舞台模式未激活时） */}
            { !isStageMode && activeTab === 'platform' && selectedReport ? (
              <div className="h-full glass rounded-2xl">
                {/* 关闭按钮 */}
                <button
                  onClick={() => setSelectedPlatform(null)}
                  className="absolute top-4 right-4 z-20 w-8 h-8 rounded-full bg-gray-100 hover:bg-gray-200 dark:bg-neutral-900 dark:hover:bg-neutral-700 flex items-center justify-center text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 transition-colors shadow-lg"
                  title={t.reportsPage.close}
                  aria-label={t.reportsPage.close}
                >
                  <FaTimes size={14} />
                </button>

                {/* 平台报告详情 */}
                <div className="h-full flex flex-col md:flex-row overflow-hidden rounded-2xl">
                  {/* 左侧：概览 */}
                  <div className="w-full md:w-1/3 p-6 md:p-8 flex flex-col border-b md:border-b-0 md:border-r border-gray-100 dark:border-neutral-800 bg-white/50 dark:bg-neutral-950/50 backdrop-blur-xl overflow-y-auto">
                    <div className={`w-12 h-12 rounded-xl flex items-center justify-center text-2xl mb-4 ${translatedPlatforms.find(p => p.id === selectedReport.platform)?.bg} ${translatedPlatforms.find(p => p.id === selectedReport.platform)?.text}`}>
                      {translatedPlatforms.find(p => p.id === selectedReport.platform)?.icon}
                    </div>

                    <h2 className="text-2xl font-bold text-gray-900 dark:text-white mb-1">{translatedPlatforms.find(p => p.id === selectedReport.platform)?.name || selectedReport.platform}</h2>
                    <div className="flex items-center gap-2 mb-6">
                      <div className="text-xs text-gray-500 dark:text-gray-400 font-mono">
                        {new Date(selectedReport.created_at).toLocaleDateString()}
                      </div>
                      <button
                        onClick={(e) => {
                          e.stopPropagation()
                          generatePlatformReport(selectedReport.platform)
                        }}
                        disabled={loadingPlatform === selectedReport.platform}
                        className="p-1.5 rounded-full hover:bg-gray-200/50 dark:hover:bg-neutral-700/50 transition-colors text-gray-500 dark:text-gray-400 disabled:opacity-50 disabled:cursor-not-allowed"
                        title={t.reportsPage.regenerateReport}
                        aria-label={t.reportsPage.regenerateReport}
                      >
                        <motion.div
                          animate={loadingPlatform === selectedReport.platform ? { rotate: 360 } : {}}
                          transition={{ repeat: Infinity, duration: 1, ease: 'linear' }}
                        >
                          <FaSync size={12} />
                        </motion.div>
                      </button>
                    </div>

                    <div className="space-y-4 flex-1">
                      <div>
                        <h3 className="text-xs font-bold text-gray-400 uppercase tracking-wider mb-2">{t.reportsPage.aiSummary}</h3>
                        <p className="text-sm text-gray-600 dark:text-gray-300 leading-relaxed">
                          {selectedReport.summary}
                        </p>
                      </div>

                      {/* 动态可视化展示区 */}
                      <div className="p-3 rounded-xl bg-gray-50 dark:bg-neutral-900 border border-gray-100 dark:border-neutral-700 h-32">
                        <div className="w-full h-full">
                          {selectedReport.platform === 'bilibili' && <BilibiliWidget data={selectedReport.card_visuals} showOverview={showOverview} defaultDanmaku={defaultDanmaku} />}
                          {selectedReport.platform === 'steam' && <SteamWidget data={selectedReport.card_visuals} showOverview={showOverview} defaultPlayerType={defaultPlayerType} />}
                          {selectedReport.platform === 'github' && <GithubWidget data={selectedReport.card_visuals} showOverview={showOverview} defaultLevel={defaultDevLevel} levelKeywords={levelKeywords} />}
                          {selectedReport.platform === 'netease' && <NeteaseWidget data={selectedReport.card_visuals} showOverview={showOverview} tenThousandSuffix={t.reportsPage.tenThousandSuffix} />}
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* 右侧：详细洞察 */}
                  <div className="flex-1 p-6 md:p-8 overflow-y-auto bg-white dark:bg-neutral-950">
                    <div className="flex items-center gap-2 mb-6">
                      <FaRobot className="text-indigo-500 text-lg" />
                      <h3 className="text-lg font-bold text-gray-900 dark:text-white">{t.reportsPage.deepInsightReport}</h3>
                    </div>

                    <div className="grid gap-4">
                      {selectedReport.insights.map((insight: string, i: number) => (
                        <motion.div
                          key={i}
                          initial={{ opacity: 0, y: 10 }}
                          animate={{ opacity: 1, y: 0 }}
                          transition={{ delay: i * 0.05 }}
                          className="flex gap-3 group"
                        >
                          <div className="flex-shrink-0 w-6 h-6 rounded-full bg-indigo-50 dark:bg-indigo-900/30 flex items-center justify-center text-indigo-500 font-bold text-xs group-hover:bg-indigo-500 group-hover:text-white transition-colors">
                            {i + 1}
                          </div>
                          <div className="flex-1 pt-0.5">
                            <p className="text-sm text-gray-600 dark:text-gray-300 leading-relaxed">
                              {insight}
                            </p>
                          </div>
                        </motion.div>
                      ))}
                    </div>
                  </div>
                </div>
              </div>
            ) : null}
          </div>

          {/* 下半部分：卡片列表区域 (40%) - 固定高度 */}
          <div className={`h-[40%] flex flex-col gap-3 relative ${isStageMode ? 'justify-end md:justify-start' : ''}`}>
            {activeTab === 'platform' && (
              <>
                {/* 平台报告标题 - 绝对定位在整个区域 */}
                <div
                  className={`absolute left-2 whitespace-nowrap pointer-events-none z-0 ${isStageMode ? 'hidden md:block' : ''}`}
                  style={{
                    top: `calc(25px - ${7.5 * titleFontSize}rem)`,
                    fontFamily: currentFont.family,
                    fontWeight: 700,
                    fontSize: `${6 * titleFontSize}rem`,
                    color: getTitleColor('primary'),
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${getTitleColor('primary')} 30%, transparent)`,
                  }}
                >
                  Character
                </div>
              </>
            )}
            {activeTab === 'comprehensive' && (
              <>
                {/* 综合报告标题 - 绝对定位在整个区域 */}
                <div
                  className={`absolute left-2 whitespace-nowrap pointer-events-none z-0 ${isStageMode ? 'hidden md:block' : ''}`}
                  style={{
                    top: `calc(25px - ${7.5 * titleFontSize}rem)`,
                    fontFamily: currentFont.family,
                    fontWeight: 700,
                    fontSize: `${6 * titleFontSize}rem`,
                    color: getTitleColor('accent'),
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${getTitleColor('accent')} 30%, transparent)`,
                  }}
                >
                  Stage
                </div>
              </>
            )}
            <div className="flex flex-col gap-3 relative z-10">
              {activeTab === 'platform' && (
                <>
                  {/* 平台报告提示条 */}
                  <motion.div
                    className={`h-[50px] ${isStageMode ? 'mb-2 md:mb-0' : ''}`}
                    initial={{ opacity: 0, x: -20 }}
                    animate={isPageReady ? { opacity: 1, x: 0 } : { opacity: 0, x: -20 }}
                    exit={{ opacity: 0, x: -20 }}
                    transition={{ duration: 0.3, ease: 'easeOut', delay: isPageReady ? 0.1 : 0 }}
                  >
                    <motion.div
                      className="w-full md:w-[24%] h-full glass rounded-xl px-4 flex items-center gap-2 shadow-sm"
                      whileHover={{ scale: 1.02 }}
                      transition={{ duration: 0.2 }}
                    >
                      {isStageMode && stageReportData?.platform ? (
                      // 舞台模式：显示当前播放的平台
                        <>
                          <div className="text-base">
                            {translatedPlatforms.find(p => p.id === stageReportData.platform)?.icon}
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="text-sm font-medium truncate text-primary-color">
                              {translatedPlatforms.find(p => p.id === stageReportData.platform)?.name || stageReportData.platform}
                            </div>
                            <div className="text-[10px] text-gray-500 dark:text-gray-400 truncate">
                              {t.reportsPage.stagePlaying}
                            </div>
                          </div>
                        </>
                      ) : (
                      // 正常模式：显示默认提示
                        <>
                          <svg className="w-5 h-5 flex-shrink-0 text-primary-color" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0a4 4 0 004-4v-4a2 2 0 012-2h4a2 2 0 012 2v4a4 4 0 01-4 4h-8z" />
                          </svg>
                          <div className="flex-1 min-w-0">
                            <div className="text-sm font-medium truncate text-primary-color">{t.reportsPage.platformReport}</div>
                            <div className="text-[10px] text-gray-500 dark:text-gray-400 truncate">{t.reportsPage.clickToView}</div>
                          </div>

                          {/* 播放全部按钮 */}
                          <button
                            onClick={startPlayAll}
                            className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                            title={t.reportsPage.playAllReports}
                          >
                            <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                              <path d="M8 5v14l11-7z" />
                            </svg>
                          </button>
                        </>
                      )}

                      {/* 舞台模式控制按钮 - 仅在舞台模式下显示 */}
                      {isStageMode && (
                        <div className="flex items-center gap-2 ml-auto">
                          {/* 刷新按钮 - 仅管理员且平台报告显示 */}
                          {isAdmin && stageReportData?.type === 'platform' && (
                            <button
                              onClick={refreshStageReport}
                              disabled={refreshingStage}
                              className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
                              title={refreshingStage ? t.reportsPage.refreshing : t.reportsPage.refreshCurrentReport}
                            >
                              <motion.svg
                                className="w-4 h-4"
                                fill="none"
                                stroke="currentColor"
                                viewBox="0 0 24 24"
                                animate={refreshingStage ? { rotate: 360 } : { rotate: 0 }}
                                transition={refreshingStage ? { repeat: Infinity, duration: 1, ease: 'linear' } : { duration: 0 }}
                              >
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                              </motion.svg>
                            </button>
                          )}

                          {/* 播放/暂停按钮 */}
                          <button
                            onClick={() => {
                            // 触发StageMode内部的暂停状态切换
                              window.dispatchEvent(new CustomEvent('stage-toggle-pause'))
                            }}
                            className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                            title={stagePaused ? t.reportsPage.continuePlay : t.reportsPage.pause}
                          >
                            {stagePaused
                              ? (
                                  <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                                    <path d="M8 5v14l11-7z" />
                                  </svg>
                                )
                              : (
                                  <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                                    <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
                                  </svg>
                                )}
                          </button>

                          {/* 关闭按钮 */}
                          <button
                            onClick={closeStageMode}
                            className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                            title={t.reportsPage.closeStage}
                          >
                            <FaTimes size={14} />
                          </button>
                        </div>
                      )}
                    </motion.div>
                  </motion.div>
                  <motion.div
                    className={`flex lg:grid lg:grid-cols-4 gap-4 overflow-x-auto lg:overflow-x-visible scrollbar-hide snap-x snap-mandatory lg:snap-none ${isStageMode ? 'hidden md:flex' : ''}`}
                    initial={{ opacity: 0 }}
                    animate={isPageReady ? { opacity: 1 } : { opacity: 0 }}
                    exit={{ opacity: 0 }}
                    transition={{ duration: 0.3, delay: isPageReady ? 0.15 : 0 }}
                  >
                    {PLATFORMS.map((platform) => {
                      const isLoading = loadingPlatform === platform.id
                      // 🚀 性能优化：使用 Map 查找，O(1) 复杂度
                      const platformReport = platformReportsMap.get(platform.id)

                      const cardIndex = PLATFORMS.findIndex(p => p.id === platform.id)

                      return (
                        <motion.div
                          key={platform.id}
                          layout
                          initial={{ opacity: 0, y: 20, scale: 0.95 }}
                          animate={isPageReady ? { opacity: 1, y: 0, scale: 1 } : { opacity: 0, y: 20, scale: 0.95 }}
                          exit={{ opacity: 0, y: -20, scale: 0.95 }}
                          transition={{
                            duration: 0.4,
                            delay: isPageReady ? cardIndex * 0.08 + 0.2 : 0,
                            ease: [0.4, 0, 0.2, 1],
                          }}
                          whileHover={{ scale: 1.02, y: -4 }}
                          whileTap={{ scale: 0.98 }}
                          className={`
                    relative aspect-[2/1] rounded-2xl overflow-hidden cursor-pointer group 
                    glass
                    hover:shadow-xl transition-shadow
                    flex-shrink-0 w-[280px] lg:w-auto snap-center
                  `}
                          style={{ willChange: 'transform, opacity' }} // 🚀 GPU加速
                          onClick={() => {
                            if (platformReport) {
                              openStageMode(platform.id)
                            }
                            else if (isAdmin) {
                              generatePlatformReport(platform.id)
                            }
                            else {
                              setToastMessage(`⛗ ${t.reportsPage.adminOnlyGenerate}`)
                            }
                          }}
                        >
                          {/* 动态背景光效 */}
                          <div className={`absolute -right-10 -top-10 w-40 h-40 bg-gradient-to-br ${platform.color} opacity-10 rounded-full blur-3xl group-hover:opacity-20 transition-opacity`} />

                          <div className="absolute inset-0 flex flex-col z-10">
                            {/* 哔哩哔哩平台特殊布局 */}
                            {platform.id === 'bilibili' ? (
                              <>
                                {/* 动态内容区 - 占满整个卡片 */}
                                <div className="flex-1 flex items-center justify-center overflow-hidden">
                                  {isLoading
                                    ? (
                                        <motion.div animate={{ rotate: 360 }} transition={{ repeat: Infinity, duration: 1 }} className={platform.text}>
                                          <FaChartPie size={20} />
                                        </motion.div>
                                      )
                                    : !platformReport
                                        ? (
                                            <div className="text-center opacity-50 group-hover:opacity-80 transition-opacity">
                                              <div className="text-[10px] text-gray-400 font-medium">{isAdmin ? t.reportsPage.clickToGenerate : t.reportsPage.noReport}</div>
                                            </div>
                                          )
                                        : (
                                            <div className="w-full h-full">
                                              <BilibiliWidget
                                                data={platformReport.card_visuals}
                                                onContentChange={contentChangeHandlers[platform.id]}
                                                showOverview={showOverview}
                                                defaultDanmaku={defaultDanmaku}
                                              />
                                            </div>
                                          )}
                                </div>

                                {/* 左下角浮动Logo - 支持动态展开 */}
                                <motion.div
                                  className="absolute bottom-3 left-3 z-20"
                                  initial={false}
                                  animate={{
                                    width: cardContents[platform.id] ? 'auto' : '32px',
                                  }}
                                  transition={{ duration: 0.3, ease: 'easeOut' }}
                                >
                                  <div
                                    className={`h-8 rounded-lg flex items-center gap-2 ${platform.text} backdrop-blur-sm shadow-lg transition-all overflow-hidden ${
                                      cardContents[platform.id]
                                        ? 'bg-white/95 dark:bg-black/95'
                                        : ''
                                    }`}
                                    style={{
                                      background: cardContents[platform.id]
                                        ? undefined
                                        : 'rgba(0, 161, 214, 0.15)',
                                      border: '1px solid rgba(0, 161, 214, 0.3)',
                                      padding: cardContents[platform.id] ? '0 8px 0 8px' : '0 8px',
                                    }}
                                  >
                                    <div className="text-base flex-shrink-0">
                                      {platform.icon}
                                    </div>
                                    <AnimatePresence>
                                      {cardContents[platform.id] && (
                                        <motion.div
                                          initial={{ opacity: 0, width: 0 }}
                                          animate={{ opacity: 1, width: 'auto' }}
                                          exit={{ opacity: 0, width: 0 }}
                                          transition={{ duration: 0.3 }}
                                          className="flex items-center gap-2 whitespace-nowrap overflow-hidden"
                                        >
                                          <span className="text-[11px] font-bold text-gray-900 dark:text-gray-100 max-w-[120px] truncate">
                                            {cardContents[platform.id].title}
                                          </span>
                                          <span
                                            className="px-1.5 py-0.5 rounded text-[8px] font-bold"
                                            style={{
                                              backgroundColor: 'rgba(0, 161, 214, 0.15)',
                                              color: '#00A1D6',
                                            }}
                                          >
                                            {cardContents[platform.id].type === 'anime' ? t.reportsPage.anime : cardContents[platform.id].type === 'tv_series' ? t.reportsPage.tvSeries : t.reportsPage.video}
                                          </span>
                                        </motion.div>
                                      )}
                                    </AnimatePresence>
                                  </div>
                                </motion.div>
                              </>
                            ) : platform.id === 'steam' ? (
                            /* Steam平台特殊布局 */
                              <>
                                {/* 动态内容区 - 占满整个卡片 */}
                                <div className="flex-1 flex items-center justify-center overflow-hidden">
                                  {isLoading
                                    ? (
                                        <motion.div animate={{ rotate: 360 }} transition={{ repeat: Infinity, duration: 1 }} className={platform.text}>
                                          <FaChartPie size={20} />
                                        </motion.div>
                                      )
                                    : !platformReport
                                        ? (
                                            <div className="text-center opacity-50 group-hover:opacity-80 transition-opacity">
                                              <div className="text-[10px] text-gray-400 font-medium">{isAdmin ? t.reportsPage.clickToGenerate : t.reportsPage.noReport}</div>
                                            </div>
                                          )
                                        : (
                                            <div className="w-full h-full">
                                              <SteamWidget
                                                data={platformReport.card_visuals}
                                                onContentChange={contentChangeHandlers[platform.id]}
                                                showOverview={showOverview}
                                                defaultPlayerType={defaultPlayerType}
                                              />
                                            </div>
                                          )}
                                </div>

                                {/* 左下角浮动Logo - 支持动态展开 */}
                                <motion.div
                                  className="absolute bottom-3 left-3 z-20"
                                  initial={false}
                                  animate={{
                                    width: cardContents[platform.id] ? 'auto' : '32px',
                                  }}
                                  transition={{ duration: 0.3, ease: 'easeOut' }}
                                >
                                  <div
                                    className={`h-8 rounded-lg flex items-center gap-2 ${platform.text} backdrop-blur-sm shadow-lg transition-all overflow-hidden ${
                                      cardContents[platform.id]
                                        ? 'bg-white/95 dark:bg-black/95'
                                        : ''
                                    }`}
                                    style={{
                                      background: cardContents[platform.id]
                                        ? undefined
                                        : 'rgba(27, 40, 56, 0.15)',
                                      border: '1px solid rgba(27, 40, 56, 0.3)',
                                      padding: cardContents[platform.id] ? '0 8px 0 8px' : '0 8px',
                                    }}
                                  >
                                    <div className="text-base flex-shrink-0">
                                      {platform.icon}
                                    </div>
                                    <AnimatePresence>
                                      {cardContents[platform.id] && (
                                        <motion.div
                                          initial={{ opacity: 0, width: 0 }}
                                          animate={{ opacity: 1, width: 'auto' }}
                                          exit={{ opacity: 0, width: 0 }}
                                          transition={{ duration: 0.3 }}
                                          className="flex items-center gap-2 whitespace-nowrap overflow-hidden"
                                        >
                                          <span className="text-[11px] font-bold text-gray-900 dark:text-gray-100 max-w-[120px] truncate">
                                            {cardContents[platform.id].title}
                                          </span>
                                        </motion.div>
                                      )}
                                    </AnimatePresence>
                                  </div>
                                </motion.div>
                              </>
                            ) : platform.id === 'github' ? (
                            /* GitHub平台特殊布局 */
                              <>
                                {/* 动态内容区 - 占满整个卡片 */}
                                <div className="flex-1 flex items-center justify-center overflow-hidden">
                                  {isLoading
                                    ? (
                                        <motion.div animate={{ rotate: 360 }} transition={{ repeat: Infinity, duration: 1 }} className={platform.text}>
                                          <FaChartPie size={20} />
                                        </motion.div>
                                      )
                                    : !platformReport
                                        ? (
                                            <div className="text-center opacity-50 group-hover:opacity-80 transition-opacity">
                                              <div className="text-[10px] text-gray-400 font-medium">{isAdmin ? t.reportsPage.clickToGenerate : t.reportsPage.noReport}</div>
                                            </div>
                                          )
                                        : (
                                            <div className="w-full h-full">
                                              <GithubWidget
                                                data={platformReport.card_visuals}
                                                onContentChange={contentChangeHandlers[platform.id]}
                                                showOverview={showOverview}
                                                defaultLevel={defaultDevLevel}
                                                levelKeywords={levelKeywords}
                                              />
                                            </div>
                                          )}
                                </div>

                                {/* 左下角浮动Logo - 支持动态展开 */}
                                <motion.div
                                  className="absolute bottom-3 left-3 z-20"
                                  initial={false}
                                  animate={{
                                    width: cardContents[platform.id] ? 'auto' : '32px',
                                  }}
                                  transition={{ duration: 0.3, ease: 'easeOut' }}
                                >
                                  <div
                                    className={`h-8 rounded-lg flex items-center gap-2 ${platform.text} backdrop-blur-sm shadow-lg transition-all overflow-hidden ${
                                      cardContents[platform.id]
                                        ? 'bg-white/95 dark:bg-black/95'
                                        : ''
                                    }`}
                                    style={{
                                      background: cardContents[platform.id]
                                        ? undefined
                                        : 'rgba(36, 41, 46, 0.15)',
                                      border: '1px solid rgba(36, 41, 46, 0.3)',
                                      padding: cardContents[platform.id] ? '0 8px 0 8px' : '0 8px',
                                    }}
                                  >
                                    <div className="text-base flex-shrink-0">
                                      {platform.icon}
                                    </div>
                                    <AnimatePresence>
                                      {cardContents[platform.id] && (
                                        <motion.div
                                          initial={{ opacity: 0, width: 0 }}
                                          animate={{ opacity: 1, width: 'auto' }}
                                          exit={{ opacity: 0, width: 0 }}
                                          transition={{ duration: 0.3 }}
                                          className="flex items-center gap-2 whitespace-nowrap overflow-hidden"
                                        >
                                          <span className="text-[11px] font-bold text-gray-900 dark:text-gray-100 max-w-[120px] truncate">
                                            {cardContents[platform.id].title}
                                          </span>
                                          {cardContents[platform.id].type !== 'repo' && (
                                            <span
                                              className="px-1.5 py-0.5 rounded text-[8px] font-bold"
                                              style={{
                                                backgroundColor: 'rgba(36, 41, 46, 0.15)',
                                                color: '#24292e',
                                              }}
                                            >
                                              {cardContents[platform.id].type}
                                            </span>
                                          )}
                                        </motion.div>
                                      )}
                                    </AnimatePresence>
                                  </div>
                                </motion.div>
                              </>
                            ) : platform.id === 'netease' ? (
                            /* 网易云音乐平台特殊布局 */
                              <>
                                {/* 动态内容区 - 占满整个卡片 */}
                                <div className="flex-1 flex items-center justify-center overflow-hidden">
                                  {isLoading
                                    ? (
                                        <motion.div animate={{ rotate: 360 }} transition={{ repeat: Infinity, duration: 1 }} className={platform.text}>
                                          <FaChartPie size={20} />
                                        </motion.div>
                                      )
                                    : !platformReport
                                        ? (
                                            <div className="text-center opacity-50 group-hover:opacity-80 transition-opacity">
                                              <div className="text-[10px] text-gray-400 font-medium">{isAdmin ? t.reportsPage.clickToGenerate : t.reportsPage.noReport}</div>
                                            </div>
                                          )
                                        : (
                                            <div className="w-full h-full">
                                              <NeteaseWidget
                                                data={platformReport.card_visuals}
                                                onContentChange={contentChangeHandlers[platform.id]}
                                                showOverview={showOverview}
                                                tenThousandSuffix={t.reportsPage.tenThousandSuffix}
                                              />
                                            </div>
                                          )}
                                </div>

                                {/* 左下角浮动Logo - 支持动态展开 */}
                                <motion.div
                                  className="absolute bottom-3 left-3 z-20"
                                  initial={false}
                                  animate={{
                                    width: cardContents[platform.id] ? 'auto' : '32px',
                                  }}
                                  transition={{ duration: 0.3, ease: 'easeOut' }}
                                >
                                  <div
                                    className={`rounded-lg flex items-center gap-2 ${platform.text} backdrop-blur-sm shadow-lg transition-all overflow-hidden ${
                                      cardContents[platform.id]
                                        ? 'bg-white/95 dark:bg-black/95'
                                        : ''
                                    }`}
                                    style={{
                                      background: cardContents[platform.id]
                                        ? undefined
                                        : 'rgba(230, 0, 38, 0.15)',
                                      border: '1px solid rgba(230, 0, 38, 0.3)',
                                      padding: cardContents[platform.id] ? '4px 8px' : '0 8px',
                                      height: cardContents[platform.id] ? 'auto' : '32px',
                                    }}
                                  >
                                    <div className="text-base flex-shrink-0">
                                      {platform.icon}
                                    </div>
                                    <AnimatePresence>
                                      {cardContents[platform.id] && (
                                        <motion.div
                                          initial={{ opacity: 0, width: 0 }}
                                          animate={{ opacity: 1, width: 'auto' }}
                                          exit={{ opacity: 0, width: 0 }}
                                          transition={{ duration: 0.3 }}
                                          className="flex flex-col gap-0.5 overflow-hidden py-0.5"
                                        >
                                          {cardContents[platform.id].titles.map((title: string, idx: number) => (
                                            <div key={idx} className="text-[10px] font-bold text-gray-900 dark:text-gray-100 max-w-[120px] truncate leading-tight">
                                              {title}
                                            </div>
                                          ))}
                                        </motion.div>
                                      )}
                                    </AnimatePresence>
                                  </div>
                                </motion.div>
                              </>
                            ) : (
                            /* 其他平台保持原有布局（空状态，不应该到达这里） */
                              <div className="p-3.5 flex flex-col justify-between h-full">
                                {/* 头部 */}
                                <div className="flex justify-between items-center">
                                  <div className="flex items-center gap-1.5">
                                    <div className={`text-sm ${platform.text}`}>
                                      {platform.icon}
                                    </div>
                                    <span className="text-[9px] font-bold text-gray-400/70 uppercase tracking-wider">
                                      {platform.name}
                                    </span>
                                  </div>
                                  {platformReport && (
                                    <div className="w-1 h-1 rounded-full bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.6)] animate-pulse" />
                                  )}
                                </div>

                                {/* 中间动态内容区 */}
                                <div className="flex-1 flex items-center justify-center overflow-hidden py-0.5">
                                  {isLoading
                                    ? (
                                        <motion.div animate={{ rotate: 360 }} transition={{ repeat: Infinity, duration: 1 }} className={platform.text}>
                                          <FaChartPie size={20} />
                                        </motion.div>
                                      )
                                    : !platformReport
                                        ? (
                                            <div className="text-center opacity-50 group-hover:opacity-80 transition-opacity">
                                              <div className="text-[10px] text-gray-400 font-medium">{isAdmin ? t.reportsPage.clickToGenerate : t.reportsPage.noReport}</div>
                                            </div>
                                          )
                                        : (
                                            <div className="w-full h-full flex items-center justify-center">
                                              <div className="text-[10px] text-gray-400">{t.reportsPage.unknownPlatform}</div>
                                            </div>
                                          )}
                                </div>
                              </div>
                            )}
                          </div>
                        </motion.div>
                      )
                    })}
                  </motion.div>
                </>
              )}

              {activeTab === 'comprehensive' && (
                <>
                  {/* 综合报告提示条 */}
                  <motion.div
                    className={`h-[50px] ${isStageMode ? 'mb-2 md:mb-0' : ''}`}
                    initial={{ opacity: 0, y: -10 }}
                    animate={isPageReady ? { opacity: 1, y: 0 } : { opacity: 0, y: -10 }}
                    transition={{ duration: 0.3, delay: isPageReady ? 0.1 : 0 }}
                  >
                    <motion.div
                      className="w-full md:w-[24%] h-full glass rounded-xl px-4 flex items-center gap-2 shadow-sm"
                      whileHover={{
                        scale: 1.02,
                        boxShadow: '0 8px 24px rgba(0, 0, 0, 0.12)',
                        transition: { duration: 0.2 },
                      }}
                    >
                      {isStageMode && stageReportData?.type === 'comprehensive' ? (
                      // 舞台模式下显示标题和控制按钮
                        <>
                          <FaMagic className="text-base flex-shrink-0 text-accent-color" />
                          <div className="flex-1 min-w-0 flex items-center justify-between gap-2">
                            <span className="text-sm font-medium truncate">
                              {stageReportData.title}
                            </span>
                            <div className="flex items-center gap-2">
                              <button
                                onClick={() => {
                                  const event = new CustomEvent('stage-toggle-pause')
                                  window.dispatchEvent(event)
                                }}
                                className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                                title={stagePaused ? t.reportsPage.continuePlay : t.reportsPage.pause}
                              >
                                {stagePaused
                                  ? (
                                      <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                                        <path d="M8 5v14l11-7z" />
                                      </svg>
                                    )
                                  : (
                                      <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                                        <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
                                      </svg>
                                    )}
                              </button>
                              <button
                                onClick={handleUserCloseStage}
                                className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                                title={t.reportsPage.closeStage}
                              >
                                <FaTimes size={14} />
                              </button>
                            </div>
                          </div>
                        </>
                      ) : (
                      // 默认状态显示输入框和生成按钮（仅管理员可见）
                        <>
                          {isAdmin ? (
                            <>
                              <motion.div
                                animate={anim.loop
                                  ? {
                                      rotate: [0, 10, -10, 10, 0],
                                      scale: [1, 1.1, 1.1, 1.1, 1],
                                    }
                                  : {}}
                                transition={{
                                  duration: 2,
                                  repeat: anim.loop ? Infinity : 0,
                                  repeatDelay: 3,
                                  ease: 'easeInOut',
                                }}
                              >
                                <FaMagic className="text-base flex-shrink-0 text-accent-color" />
                              </motion.div>
                              <div className="flex-1 min-w-0 flex items-center gap-2">
                                <input
                                  type="text"
                                  value={customStyle}
                                  onChange={e => setCustomStyle(e.target.value)}
                                  placeholder={t.reportsPage.styleDescPlaceholder}
                                  className="clean-input flex-1 min-w-0 text-sm"
                                />
                                <motion.button
                                  onClick={generateComprehensiveReport}
                                  disabled={loadingPlatform === 'comprehensive'}
                                  className="flex-shrink-0 h-[26px] px-3 text-white rounded text-xs font-medium transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
                                  style={{
                                    background: `linear-gradient(135deg, var(--color-accent), var(--color-secondary))`,
                                  }}
                                  whileHover={{
                                    scale: 1.08,
                                    transition: { duration: 0.2 },
                                  }}
                                  whileTap={{
                                    scale: 0.92,
                                    transition: { duration: 0.1 },
                                  }}
                                >
                                  {loadingPlatform === 'comprehensive'
                                    ? (
                                        <>
                                          <motion.div animate={{ rotate: 360 }} transition={{ repeat: Infinity, duration: 1, ease: 'linear' }}>
                                            <FaSync size={10} />
                                          </motion.div>
                                          <span>{t.reportsPage.generating}</span>
                                        </>
                                      )
                                    : (
                                        <>
                                          <motion.div
                                            animate={anim.loop
                                              ? {
                                                  scale: [1, 1.2, 1],
                                                  rotate: [0, 5, -5, 0],
                                                }
                                              : {}}
                                            transition={{
                                              duration: 1.5,
                                              repeat: anim.loop ? Infinity : 0,
                                              repeatDelay: 2,
                                            }}
                                          >
                                            <FaMagic size={10} />
                                          </motion.div>
                                          <span>{t.reportsPage.generate}</span>
                                        </>
                                      )}
                                </motion.button>
                              </div>
                            </>
                          ) : (
                          // 非管理员显示提示
                            <>
                              <FaMagic className="text-base flex-shrink-0 opacity-50 text-accent-color" />
                              <div className="flex-1 min-w-0">
                                <div className="text-sm font-medium text-gray-600 dark:text-gray-400">{t.reportsPage.comprehensiveReport}</div>
                                <div className="text-[10px] text-gray-500 dark:text-gray-500">{t.reportsPage.adminOnlyGenerateHint}</div>
                              </div>
                            </>
                          )}
                        </>
                      )}
                    </motion.div>
                  </motion.div>
                  <motion.div
                    className={`flex lg:grid lg:grid-cols-4 gap-4 overflow-x-auto lg:overflow-x-visible scrollbar-hide snap-x snap-mandatory lg:snap-none ${isStageMode ? 'hidden md:flex' : ''}`}
                    initial={{ opacity: 0 }}
                    animate={isPageReady ? { opacity: 1 } : { opacity: 0 }}
                    exit={{ opacity: 0 }}
                    transition={{ duration: 0.3, delay: isPageReady ? 0.15 : 0 }}
                  >
                    <AnimatePresence mode="popLayout">
                      {displayedComprehensiveReports.length > 0 ? (
                        displayedComprehensiveReports.map((compReport: any, index: number) => (
                          <ComprehensiveReportCard
                            key={compReport.id}
                            compReport={compReport}
                            index={index}
                            onOpen={(analysis: any, id: number, createdAt: string) => {
                              // 打开综合报告的舞台模式
                              // 从所有平台报告中提取 library_items
                              const allLibraryItems: Array<{ title: string, cover?: string, type: string, platform?: string }> = []

                              if (report?.platform_reports) {
                                report.platform_reports.forEach((platformReport) => {
                                  const cardVisuals: any = platformReport.card_visuals
                                  if (cardVisuals && typeof cardVisuals === 'object') {
                                    const items = cardVisuals.library_items
                                    if (Array.isArray(items)) {
                                      items.forEach((item: any) => {
                                        allLibraryItems.push({
                                          title: item.title,
                                          cover: item.cover,
                                          type: item.type || 'content',
                                          platform: platformReport.platform,
                                        })
                                      })
                                    }
                                  }
                                })
                              }

                              setStageReportData({
                                type: 'comprehensive',
                                综合分析: analysis,
                                library_items: allLibraryItems,
                                title: analysis.visual_style || t.reportsPage.allPlatformReport,
                              })
                              setIsStageMode(true)
                            }}
                          />
                        ))
                      ) : (
                        <EmptyComprehensiveReport isAdmin={isAdmin} />
                      )}
                    </AnimatePresence>
                  </motion.div>
                </>
              )}
            </div>
          </div>
        </div>
      </div>
    </AnimatedView>
  )
}
