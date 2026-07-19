import type { ReactNode } from 'react'
import type { ToastType } from '../components/Toast'
import type { WidgetConfig } from '../components/WidgetGrid'
import type { SecondaryNavItem } from '../contexts/NavigationContext'
import {
  FaGithub,
  FaMagic,
  FaSteam,
  FaSync,
  FaTimes,
  FaXbox,
  FaXTwitter,
  LuGlobe,
  SiBangumi,
  SiBilibili,
  SiDiscord,
  SiMyanimelist,
  SiNeteasecloudmusic,
  SiPlaystation,
} from '@lib/icons'

import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  memo,

  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import AnimatedView from '../components/AnimatedView'
import StageMode from '../components/StageMode'
import Toast from '../components/Toast'
import { ReportCardWidget } from '../components/widgets/ReportCardWidget'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useSecondaryNav } from '../contexts/NavigationContext'
import {
  usePageReady,
  useReportsScheduler,
} from '../hooks/animation'
import { useAnimationLevel } from '../hooks/useAnimationLevel'
import {
  useResolvedTitleColor,
  useTitleFont,
} from '../hooks/useTitleFont'
import { getCSRFToken } from '../utils/csrf'
import { notifyRecentActivityUpdated } from '../utils/recentActivity'
import { invalidateLatestReportCache } from '../utils/requestDedup'
import { hasSessionHint } from '../utils/sessionDetection'
import { ComprehensiveReportCard } from './reports/ComprehensiveReportCard'
import { EmptyComprehensiveReport } from './reports/EmptyComprehensiveReport'
import {
  REPORT_CARD_FLEX_BASIS,
  REPORT_CAROUSEL_CSS_VARS,
} from './reports/types'

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
    languages?: { name: string; percentage: number }[]
    soul_color?: string
    mood_keywords?: string[]
    taste_profile?: string
    status_counts?: Record<string, number>
    subject_type_distribution?: Record<string, number>
    collection_type_distribution?: Record<string, number>
    score_distribution?: Record<string, number>
    favorite_tags?: Record<string, number>
    top_subjects?: Array<{
      subject_id?: number
      title?: string
      rate?: number
      subject_type?: string
      collection_type?: string
      cover?: string
    }>
    library_items?: Array<{
      title: string
      cover?: string
      type: string
      platform?: string
      rate?: number
    }>
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
  {
    id: 'bangumi',
    name: 'Bangumi',
    icon: <SiBangumi />,
    color: 'from-rose-400 to-pink-500',
    bg: 'bg-rose-50/10 dark:bg-rose-900/10',
    text: 'text-rose-500',
    border: 'border-rose-200/20 dark:border-rose-800/20',
    widgetType: 'book',
  },
  {
    id: 'mal',
    name: 'MyAnimeList',
    icon: <SiMyanimelist />,
    color: 'from-blue-600 to-indigo-700',
    bg: 'bg-blue-50/10 dark:bg-blue-900/10',
    text: 'text-blue-600 dark:text-blue-400',
    border: 'border-blue-200/20 dark:border-blue-800/20',
    widgetType: 'book',
  },
  {
    id: 'x',
    name: 'X',
    icon: <FaXTwitter />,
    color: 'from-gray-800 to-black',
    bg: 'bg-gray-50/10 dark:bg-neutral-900/10',
    text: 'text-gray-900 dark:text-gray-100',
    border: 'border-gray-200/20 dark:border-neutral-700/20',
    widgetType: 'feed',
  },
  {
    id: 'discord',
    name: 'Discord',
    icon: <SiDiscord />,
    color: 'from-indigo-500 to-indigo-700',
    bg: 'bg-indigo-50/10 dark:bg-indigo-900/10',
    text: 'text-indigo-500',
    border: 'border-indigo-200/20 dark:border-indigo-800/20',
    widgetType: 'social',
  },
  {
    id: 'xbox',
    name: 'Xbox',
    icon: <FaXbox />,
    color: 'from-green-600 to-green-800',
    bg: 'bg-green-50/10 dark:bg-green-900/10',
    text: 'text-[#107C10]',
    border: 'border-green-200/20 dark:border-green-800/20',
    widgetType: 'gauge',
  },
  {
    id: 'psn',
    name: 'PlayStation',
    icon: <SiPlaystation />,
    color: 'from-blue-600 to-blue-800',
    bg: 'bg-blue-50/10 dark:bg-blue-900/10',
    text: 'text-[#0070D1]',
    border: 'border-blue-200/20 dark:border-blue-800/20',
    widgetType: 'gauge',
  },
]

const PLATFORM_NAME_TO_ID: Record<string, string> = {
  bilibili: 'bilibili',
  steam: 'steam',
  github: 'github',
  bangumi: 'bangumi',
  mal: 'mal',
  myanimelist: 'mal',
  'my anime list': 'mal',
  x: 'x',
  twitter: 'x',
  'x (twitter)': 'x',
  discord: 'discord',
  xbox: 'xbox',
  psn: 'psn',
  playstation: 'psn',
  'netease music': 'netease',
  'netease cloud music': 'netease',
  'netease cloudmusic': 'netease',
  '网易云': 'netease',
  '网易云音乐': 'netease',
}

function resolvePlatformId(platformName: string): string | null {
  const normalizedName = platformName.trim().toLowerCase().replace(/\s+/g, ' ')
  return PLATFORM_NAME_TO_ID[normalizedName] ?? null
}

function PlatformReportGeneratingSpin({
  className = '',
}: {
  className?: string
}) {
  return <FaSync size={18} className={`${className} animate-spin`} />
}

const STAGE_PLACEHOLDER_EASE = [0.4, 0, 0.2, 1] as const
const STAGE_PLACEHOLDER_TRANSITION = {
  duration: 0.28,
  ease: STAGE_PLACEHOLDER_EASE,
}

/** 舞台模式播放中：入口卡片简洁占位 */
const StagePlayingCardPlaceholder = memo(({
  icon,
  name,
  textClass,
  borderClass,
  label,
}: {
  icon: ReactNode
  name: string
  textClass: string
  borderClass: string
  label: string
}) => {
  return (
    <motion.div
      className="absolute inset-0 flex items-center justify-center gap-3 px-6"
      initial={{ opacity: 0, scale: 0.97 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.97 }}
      transition={STAGE_PLACEHOLDER_TRANSITION}
    >
      <div
        className={`w-10 h-10 shrink-0 rounded-xl flex items-center justify-center text-lg border bg-white/60 dark:bg-white/5 ${textClass} ${borderClass}`}
      >
        {icon}
      </div>
      <div className="min-w-0">
        <div className="text-sm font-semibold tracking-tight text-gray-900 dark:text-gray-100 truncate">
          {name}
        </div>
        <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
          {label}
        </div>
      </div>
    </motion.div>
  )
})

// 🚀 性能优化：将综合报告卡片提取为独立的 memo 组件
export default function Reports() {
  // 🆕 初始化报告页调度器（Visibility + Interval + RAF + DOMBatch）
  useReportsScheduler()

  const { t } = useI18n()
  const isPageReady = usePageReady()
  // 🆕 标题字体 Hook
  const { currentFont, titleFontSize } = useTitleFont()
  // 自适应色对齐 Tapp 音乐播放器歌词：对比度推导
  const titleColorPrimary = useResolvedTitleColor('primary')
  const titleColorAccent = useResolvedTitleColor('accent')

  // 二级导航项配置
  const navItems: SecondaryNavItem[] = useMemo(
    () => [
      {
        id: 'platform',
        icon: (
          <svg
            className="w-5 h-5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0a4 4 0 004-4v-4a2 2 0 012-2h4a2 2 0 012 2v4a4 4 0 01-4 4h-8z"
            />
          </svg>
        ),
        label: t.nav.platformReport,
        title: t.nav.platformReport,
        ariaLabel: t.nav.showPlatformReport,
      },
      {
        id: 'comprehensive',
        icon: (
          <svg
            className="w-5 h-5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 8v13m0-13V6a2 2 0 112 2h-2zm0 0V5.5A2.5 2.5 0 109.5 8H12zm-7 4h14M5 12a2 2 0 110-4h14a2 2 0 110 4M5 12v7a2 2 0 002 2h10a2 2 0 002-2v-7"
            />
          </svg>
        ),
        label: t.nav.comprehensiveReport,
        title: t.nav.comprehensiveReport,
        ariaLabel: t.nav.showComprehensiveReport,
      },
    ],
    [t],
  )

  // 使用二级导航 Hook
  const {
    activeId: activeTab,
    setActiveId: _setActiveTab,
    setExpanded,
  } = useSecondaryNav({
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

    window.addEventListener(
      'nav-expand-secondary',
      handleExpandSecondary as EventListener,
    )
    return () => {
      window.removeEventListener(
        'nav-expand-secondary',
        handleExpandSecondary as EventListener,
      )
    }
  }, [setExpanded])

  const [loadingPlatform, setLoadingPlatform] = useState<string | null>(null)
  const [report, setReport] = useState<CrossPlatformReport | null>(null)
  const [customStyle, setCustomStyle] = useState<string>('')
  // 🚀 性能优化：防抖处理用户输入
  const debouncedCustomStyle = useDebounce(customStyle, 300)
  // 🆕 动画级别控制，用于装饰性动画
  const anim = useAnimationLevel()
  const [isAdmin, setIsAdmin] = useState(false)
  const [comprehensiveReports, setComprehensiveReports] = useState<any[]>([]) // 所有综合报告列表
  const [enabledPlatformIds, setEnabledPlatformIds] = useState<string[]>([])
  const [platformVisibilityReady, setPlatformVisibilityReady] =
    useState(false)

  // 🎭 舞台模式状态
  const [isStageMode, setIsStageMode] = useState(false)
  const [stagePaused, setStagePaused] = useState(false) // 舞台模式暂停状态
  const [refreshingStage, setRefreshingStage] = useState(false) // 刷新舞台报告加载状态
  const [toastMessage, setToastMessage] = useState<string>('') // Toast消息
  const [toastType, setToastType] = useState<ToastType>('info')
  const [playAllMode, setPlayAllMode] = useState(false) // 播放所有模式
  const [_playAllQueue, setPlayAllQueue] = useState<string[]>([]) // 播放队列
  const playAllQueueRef = useRef<string[]>([]) // 用ref保存队列，避免闭包问题
  const [stageReportData, setStageReportData] = useState<{
    platform?: string
    summary?: string
    insights?: string[]
    card_visuals?: any
    // 综合报告字段
    综合分析?: any
    library_items?: Array<{
      title: string
      cover?: string
      type: string
      platform?: string
    }>
    type?: 'platform' | 'comprehensive'
    title?: string // 综合报告标题
  } | null>(null)

  const showToastMessage = useCallback(
    (message: string, type: ToastType = 'info') => {
      setToastType(type)
      setToastMessage(message)
    },
    [],
  )

  // i18n: 翻译后的平台配置
  const translatedPlatforms = useMemo(
    () =>
      PLATFORMS.map((p) => ({
        ...p,
        name:
          p.id === 'netease'
            ? t.reportsPage.neteaseMusic
            : p.id === 'bilibili'
              ? t.reportsPage.bilibili
              : p.id === 'steam'
                ? t.reportsPage.steam
                : p.id === 'github'
                  ? t.reportsPage.github
                  : p.name,
      })),
    [
      t.reportsPage.neteaseMusic,
      t.reportsPage.bilibili,
      t.reportsPage.steam,
      t.reportsPage.github,
    ],
  )

  // 🆕 按 enabledPlatformIds 的顺序排列（该顺序来自后端 platform_order 配置），
  // 使报告页平台卡片的出现顺序与设置里的平台排序一致
  const visiblePlatforms = useMemo(
    () =>
      enabledPlatformIds
        .map((id) => translatedPlatforms.find((platform) => platform.id === id))
        .filter(
          (platform): platform is (typeof translatedPlatforms)[number] =>
            Boolean(platform),
        ),
    [enabledPlatformIds, translatedPlatforms],
  )

  const hasEnabledPlatforms = visiblePlatforms.length > 0

  // 🚀 性能优化：监听舞台暂停状态
  useEffect(() => {
    const handlePauseStateChange = (e: CustomEvent<{ isPaused: boolean }>) => {
      setStagePaused(e.detail.isPaused)
    }

    window.addEventListener(
      'stage-pause-state-change',
      handlePauseStateChange as EventListener,
    )

    return () => {
      window.removeEventListener(
        'stage-pause-state-change',
        handlePauseStateChange as EventListener,
      )
    }
  }, [])

  // 🚀 性能优化：缓存平台报告映射，避免重复查找
  const platformReportsMap = useMemo(() => {
    const map = new Map<string, PlatformReport>()
    report?.platform_reports?.forEach((r) => map.set(r.platform, r))
    return map
  }, [report?.platform_reports])

  // 🚀 性能优化：稳定的 ReportCardWidget config，避免每次渲染新建对象打破 memo
  const platformWidgetConfigs = useMemo(() => {
    const map: Record<string, WidgetConfig> = {}
    PLATFORMS.forEach((p) => {
      map[p.id] = { config: { platformId: p.id } } as WidgetConfig
    })
    return map
  }, [])

  // 🚀 性能优化：缓存显示的综合报告列表（限制20个）
  const displayedComprehensiveReports = useMemo(
    () => comprehensiveReports.slice(0, 20),
    [comprehensiveReports],
  )

  // 🎭 打开舞台模式
  const openStageMode = useCallback(
    (platformId: string) => {
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
    },
    [platformReportsMap],
  )

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

  useEffect(() => {
    if (!platformVisibilityReady) {
      return
    }

    if (
      isStageMode &&
      stageReportData?.type === 'platform' &&
      stageReportData.platform &&
      !enabledPlatformIds.includes(stageReportData.platform)
    ) {
      closeStageMode()
    }
  }, [
    closeStageMode,
    enabledPlatformIds,
    isStageMode,
    platformVisibilityReady,
    stageReportData,
  ])

  // 🎭 开始播放所有平台
  const startPlayAll = useCallback(() => {
    // 获取所有有报告的平台
    const platformsWithReports = visiblePlatforms.filter((p) =>
      platformReportsMap.has(p.id),
    ).map((p) => p.id)

    if (platformsWithReports.length === 0) {
      showToastMessage(t.reportsPage.noPlatformReports, 'error')
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
  }, [platformReportsMap, t.reportsPage.noPlatformReports, visiblePlatforms])

  // 🎭 播放下一个平台（播放所有模式）
  const playNextPlatform = useCallback(() => {
    if (playAllQueueRef.current.length === 0) {
      // 所有平台播放完毕，退出舞台模式
      closeStageMode()
      showToastMessage(t.reportsPage.allPlaybackComplete, 'success')
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

  // 将单个平台报告合并进列表，避免整表替换导致其它卡片重渲染
  const mergePlatformReport = useCallback((updated: PlatformReport) => {
    setReport((prev) => {
      const existing = prev?.platform_reports ?? []
      const idx = existing.findIndex((r) => r.platform === updated.platform)
      const platform_reports =
        idx >= 0
          ? existing.map((r, i) => (i === idx ? updated : r))
          : [...existing, updated]

      return {
        platform_reports,
        综合分析: prev?.综合分析 || null,
        created_at: updated.created_at || prev?.created_at || new Date().toISOString(),
      }
    })
  }, [])

  // 🎭 刷新当前舞台模式的平台报告
  const refreshStageReport = useCallback(async () => {
    if (!stageReportData?.platform || stageReportData.type !== 'platform') {
      return
    }

    const platformId = stageReportData.platform
    const platformName =
      translatedPlatforms.find((p) => p.id === platformId)?.name || platformId

    setRefreshingStage(true)

    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        showToastMessage(t.reportsPage.getTokenFailed, 'error')
        setRefreshingStage(false)
        return
      }

      showToastMessage(
        t.reportsPage.refreshingReport.replace('{platform}', platformName),
        'success',
      )

      // 1. 刷新该平台的数据
      try {
        const fetchResponse = await fetch(
          `${API_URL}/api/profile/fetch-platform`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-CSRF-Token': csrfToken,
            },
            credentials: 'include',
            body: JSON.stringify({ platform: platformId }),
          },
        )
        const fetchBody = await fetchResponse.json().catch(() => null)
        if (fetchResponse.ok && fetchBody?.success !== false) {
          notifyRecentActivityUpdated()
        }
      } catch (fetchErr) {
        console.warn(`Refresh ${platformId} data request error:`, fetchErr)
      }

      // 2. 生成新报告（仅当前平台）
      const response = await fetch(`${API_URL}/api/reports/platform`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ platforms: [platformId] }),
      })

      if (!response.ok) throw new Error(t.reportsPage.generateFailed)

      // 解析生成结果，透出后端给出的跳过原因（数据未抓取/为空等）
      const genBody = await response.json().catch(() => null)
      const skippedReason =
        genBody?.success === false
          ? genBody?.message
          : Array.isArray(genBody?.skipped)
            ? genBody.skipped.find((s: any) => s?.platform === platformId)
                ?.reason
            : null
      if (skippedReason) {
        showToastMessage(skippedReason, 'error')
        return
      }

      // 3. 用生成接口返回的单份报告做局部 merge，不拉全量 latest
      const updatedPlatformReport: PlatformReport | undefined = Array.isArray(
        genBody?.reports,
      )
        ? genBody.reports.find((r: PlatformReport) => r.platform === platformId)
        : undefined

      if (updatedPlatformReport) {
        // Drop home-widget empty cache so own-page ReportCards re-fetch content
        invalidateLatestReportCache()
        mergePlatformReport(updatedPlatformReport)
        setStageReportData({
          type: 'platform',
          platform: platformId,
          summary: updatedPlatformReport.summary,
          insights: updatedPlatformReport.insights,
          card_visuals: updatedPlatformReport.card_visuals,
        })
        showToastMessage(
          t.reportsPage.reportRefreshSuccess.replace(
            '{platform}',
            platformName,
          ),
          'success',
        )
      } else {
        showToastMessage(t.reportsPage.reportRefreshNoData, 'error')
      }
    } catch (err) {
      console.error('Refresh stage report failed:', err)
      showToastMessage(
        t.reportsPage.refreshReportFailed.replace('{platform}', platformName),
        'error',
      )
    } finally {
      setRefreshingStage(false)
    }
  }, [stageReportData, mergePlatformReport, t.reportsPage, translatedPlatforms, showToastMessage])

  // 使用 AuthContext 获取管理员状态
  const {
    isAdmin: authIsAdmin,
    isAuthenticated,
    hasChecked,
    checkAuth,
  } = useAuth()

  // 智能检测：如果有登录迹象且未检查过，触发认证检查
  useEffect(() => {
    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }
  }, [hasChecked, checkAuth])

  useEffect(() => {
    let cancelled = false

    const fetchEnabledPlatforms = async () => {
      try {
        const response = await fetch(`${API_URL}/api/config/public`, {
          credentials: 'include',
        })

        if (!response.ok) {
          throw new Error(`Failed to fetch public config: ${response.status}`)
        }

        const data = await response.json()
        if (!Array.isArray(data.platforms)) {
          throw new TypeError('Public config does not contain platforms')
        }

        const nextPlatformIds = data.platforms
          .filter((platform: any) => platform?.enabled)
          .map((platform: any) =>
            typeof platform?.name === 'string'
              ? resolvePlatformId(platform.name)
              : null,
          )
          .filter((platformId: string | null): platformId is string =>
            Boolean(platformId),
          )

        if (!cancelled) {
          setEnabledPlatformIds(nextPlatformIds)
          setPlatformVisibilityReady(true)
        }
      } catch (err) {
        console.error('获取已启用数据平台失败:', err)

        if (!cancelled) {
          setEnabledPlatformIds(PLATFORMS.map((platform) => platform.id))
          setPlatformVisibilityReady(true)
        }
      }
    }

    fetchEnabledPlatforms()

    return () => {
      cancelled = true
    }
  }, [])

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
          if (
            comprehensiveData.success &&
            comprehensiveData.reports &&
            comprehensiveData.reports.length > 0
          ) {
            // 🚀 性能优化：并行获取所有综合报告的详情（批量请求）
            const detailPromises = comprehensiveData.reports.map(
              async (report: any) => {
                try {
                  const detailResponse = await fetch(
                    `${API_URL}/api/reports/comprehensive/${report.id}`,
                    {
                      credentials: 'include',
                    },
                  )
                  if (detailResponse.ok) {
                    const detailData = await detailResponse.json()
                    if (detailData.success && detailData.report) {
                      return {
                        ...report,
                        综合分析: detailData.report.综合分析 || null,
                      }
                    }
                  }
                } catch (err) {
                  console.error(`获取报告 ${report.id} 详情失败:`, err)
                }
                return report
              },
            )

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
      } catch (err) {
        console.error('获取最新报告失败:', err)
      }
    }

    fetchLatestReport()
  }, [])

  // 生成单个平台报告 - 性能优化：使用 useCallback
  const generatePlatformReport = useCallback(
    async (platformId: string) => {
      setLoadingPlatform(platformId)
      try {
        const csrfToken = await getCSRFToken(true)
        if (!csrfToken) return

        // 1. 先刷新该平台的数据
        let fetchWarning: string | null = null
        try {
          const fetchResponse = await fetch(
            `${API_URL}/api/profile/fetch-platform`,
            {
              method: 'POST',
              headers: {
                'Content-Type': 'application/json',
                'X-CSRF-Token': csrfToken,
              },
              credentials: 'include',
              body: JSON.stringify({ platform: platformId }),
            },
          )

          // 后端现在会在数据为空时返回 success:false + 可读原因
          const fetchBody = await fetchResponse.json().catch(() => null)
          if (!fetchResponse.ok || fetchBody?.success === false) {
            fetchWarning =
              fetchBody?.message ||
              `刷新 ${platformId} 数据失败，尝试使用现有数据生成报告`
            console.warn(fetchWarning)
          } else {
            notifyRecentActivityUpdated()
          }
        } catch (fetchErr) {
          console.warn(`刷新 ${platformId} 数据请求出错:`, fetchErr)
        }

        // 2. 生成报告（仅当前平台）
        const response = await fetch(`${API_URL}/api/reports/platform`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': csrfToken,
          },
          credentials: 'include',
          body: JSON.stringify({ platforms: [platformId] }),
        })

        if (!response.ok) throw new Error(t.reportsPage.generateFailed)

        // 解析生成结果，把后端给出的跳过原因透出给用户
        const genBody = await response.json().catch(() => null)
        if (genBody && genBody.success === false) {
          showToastMessage(
            genBody.message || fetchWarning || t.reportsPage.generateFailed,
            'error',
          )
          return
        }
        const skippedReason = Array.isArray(genBody?.skipped)
          ? genBody.skipped.find((s: any) => s?.platform === platformId)?.reason
          : null
        if (skippedReason) {
          showToastMessage(skippedReason, 'error')
          return
        }

        // 3. 局部 merge 生成结果，避免整表替换其它平台报告
        const updated: PlatformReport | undefined = Array.isArray(
          genBody?.reports,
        )
          ? genBody.reports.find((r: PlatformReport) => r.platform === platformId)
          : undefined
        if (updated) {
          invalidateLatestReportCache()
          mergePlatformReport(updated)
        }
      } catch (err) {
        console.error('Generate platform report failed:', err)
      } finally {
        setLoadingPlatform(null)
      }
    },
    [t.reportsPage.generateFailed, mergePlatformReport, showToastMessage],
  )

  // 生成综合分析 (基于已有平台报告) - 性能优化：使用 useCallback
  const generateComprehensiveReport = useCallback(async () => {
    setLoadingPlatform('comprehensive')
    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) return

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
        const listResponse = await fetch(
          `${API_URL}/api/reports/comprehensive/list`,
          {
            credentials: 'include',
          },
        )

        if (listResponse.ok) {
          const listData = await listResponse.json()
          if (listData.success && listData.reports) {
            // 并行获取所有综合报告的详情
            const detailPromises = listData.reports.map(async (report: any) => {
              try {
                const detailResponse = await fetch(
                  `${API_URL}/api/reports/comprehensive/${report.id}`,
                  {
                    credentials: 'include',
                  },
                )
                if (detailResponse.ok) {
                  const detailData = await detailResponse.json()
                  if (detailData.success && detailData.report) {
                    return {
                      ...report,
                      综合分析: detailData.report.综合分析 || null,
                    }
                  }
                }
              } catch (err) {
                console.error(`Get report ${report.id} details failed:`, err)
              }
              return report
            })

            const reportsWithDetails = await Promise.all(detailPromises)
            setComprehensiveReports(reportsWithDetails)
          }
        }
      } else if (!data.success && data.message) {
        console.warn(data.message)
        alert(data.message)
      }
    } catch (err) {
      console.error('Generate comprehensive report failed:', err)
      alert(
        err instanceof Error ? err.message : t.reportsPage.generateFailedRetry,
      )
    } finally {
      setLoadingPlatform(null)
    }
  }, [
    debouncedCustomStyle,
    t.reportsPage.generateFailed,
    t.reportsPage.generateFailedRetry,
  ])

  return (
    <AnimatedView className="min-h-screen md:h-screen md:overflow-hidden">
      {/* Toast提示 */}
      {toastMessage && (
        <Toast
          message={toastMessage}
          type={toastType}
          onClose={() => setToastMessage('')}
        />
      )}

      {/* 🎭 舞台模式组件 - 固定在顶部 */}
      <StageMode
        isOpen={isStageMode}
        onClose={handleUserCloseStage}
        reportData={stageReportData}
        onRefresh={refreshStageReport}
        playAllMode={playAllMode}
      />
      <div className="flex flex-col pt-20 pb-24 md:pb-6 px-3 xs:px-4 sm:px-6 min-h-dvh md:h-dvh">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col gap-4 p-2">
          {/* 上半部分：报告详情展示区域 - 移动端弹性占位抨卡片到底部，桌面端60% */}
          <div className="flex-1 md:flex-none md:h-[60%] rounded-2xl relative overflow-hidden" />

          {/* 下半部分：卡片列表区域 - 移动端/桌面端都在下半部分 */}
          <div className="md:h-[40%] flex flex-col gap-3 relative justify-end md:justify-start">
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
                    color: titleColorPrimary,
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorPrimary} 30%, transparent)`,
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
                    color: titleColorAccent,
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorAccent} 30%, transparent)`,
                  }}
                >
                  Stage
                </div>
              </>
            )}
            <div className="flex flex-col gap-3 relative z-10">
              {activeTab === 'platform' && platformVisibilityReady && (
                <>
                  {/* 平台报告提示条 */}
                  <motion.div
                    className={`h-12.5 ${isStageMode ? 'mb-2 md:mb-0' : ''}`}
                    initial={{ opacity: 0, x: -20 }}
                    animate={
                      isPageReady
                        ? { opacity: 1, x: 0 }
                        : { opacity: 0, x: -20 }
                    }
                    exit={{ opacity: 0, x: -20 }}
                    transition={{
                      duration: 0.3,
                      ease: 'easeOut',
                      delay: isPageReady ? 0.1 : 0,
                    }}
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
                            {
                              translatedPlatforms.find(
                                (p) => p.id === stageReportData.platform,
                              )?.icon
                            }
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="text-sm font-medium truncate text-primary-color">
                              {translatedPlatforms.find(
                                (p) => p.id === stageReportData.platform,
                              )?.name || stageReportData.platform}
                            </div>
                            <div className="text-[10px] text-gray-500 dark:text-gray-400 truncate">
                              {t.reportsPage.stagePlaying}
                            </div>
                          </div>
                        </>
                      ) : (
                        // 正常模式：显示默认提示
                        <>
                          <svg
                            className="w-5 h-5 shrink-0 text-primary-color"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0a4 4 0 004-4v-4a2 2 0 012-2h4a2 2 0 012 2v4a4 4 0 01-4 4h-8z"
                            />
                          </svg>
                          <div className="flex-1 min-w-0">
                            <div className="text-sm font-medium truncate text-primary-color">
                              {t.reportsPage.platformReport}
                            </div>
                            <div className="text-[10px] text-gray-500 dark:text-gray-400 truncate">
                              {hasEnabledPlatforms
                                ? t.reportsPage.clickToView
                                : t.reportsPage.noEnabledPlatforms}
                            </div>
                          </div>

                          {/* 播放全部按钮 */}
                          {hasEnabledPlatforms && (
                            <button
                              onClick={startPlayAll}
                              className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                              title={t.reportsPage.playAllReports}
                            >
                              <svg
                                className="w-4 h-4"
                                fill="currentColor"
                                viewBox="0 0 24 24"
                              >
                                <path d="M8 5v14l11-7z" />
                              </svg>
                            </button>
                          )}
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
                              title={
                                refreshingStage
                                  ? t.reportsPage.refreshing
                                  : t.reportsPage.refreshCurrentReport
                              }
                            >
                              <motion.svg
                                className="w-4 h-4"
                                fill="none"
                                stroke="currentColor"
                                viewBox="0 0 24 24"
                                animate={
                                  refreshingStage
                                    ? { rotate: 360 }
                                    : { rotate: 0 }
                                }
                                transition={
                                  refreshingStage
                                    ? {
                                        repeat: Infinity,
                                        duration: 1,
                                        ease: 'linear',
                                      }
                                    : { duration: 0 }
                                }
                              >
                                <path
                                  strokeLinecap="round"
                                  strokeLinejoin="round"
                                  strokeWidth={2}
                                  d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                                />
                              </motion.svg>
                            </button>
                          )}

                          {/* 播放/暂停按钮 */}
                          <button
                            onClick={() => {
                              // 触发StageMode内部的暂停状态切换
                              window.dispatchEvent(
                                new CustomEvent('stage-toggle-pause'),
                              )
                            }}
                            className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                            title={
                              stagePaused
                                ? t.reportsPage.continuePlay
                                : t.reportsPage.pause
                            }
                          >
                            {stagePaused ? (
                              <svg
                                className="w-4 h-4"
                                fill="currentColor"
                                viewBox="0 0 24 24"
                              >
                                <path d="M8 5v14l11-7z" />
                              </svg>
                            ) : (
                              <svg
                                className="w-4 h-4"
                                fill="currentColor"
                                viewBox="0 0 24 24"
                              >
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
                  {hasEnabledPlatforms && (
                    <motion.div
                      className={`${isStageMode ? 'hidden md:flex' : 'flex'} relative left-1/2 w-dvw max-w-none -translate-x-1/2 gap-4 overflow-x-auto scrollbar-hide snap-x snap-mandatory pt-8 pb-12 -mt-7 -mb-11 pr-3 xs:pr-4 sm:pr-6 md:pr-8 ${REPORT_CAROUSEL_CSS_VARS}`}
                      style={
                        {
                          paddingLeft:
                            'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.5rem)',
                          scrollPaddingLeft:
                            'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.5rem)',
                        } as React.CSSProperties
                      }
                      initial={{ opacity: 0 }}
                      animate={isPageReady ? { opacity: 1 } : { opacity: 0 }}
                      exit={{ opacity: 0 }}
                      transition={{
                        duration: 0.3,
                        delay: isPageReady ? 0.15 : 0,
                      }}
                    >
                      {visiblePlatforms.map((platform, cardIndex) => {
                        const isLoading = loadingPlatform === platform.id
                        // 🚀 性能优化：使用 Map 查找，O(1) 复杂度
                        const platformReport = platformReportsMap.get(platform.id)
                        // 舞台模式正在播放该平台：入口卡片改为占位提示
                        const isPlayingOnStage =
                          isStageMode &&
                          stageReportData?.type === 'platform' &&
                          stageReportData.platform === platform.id

                        return (
                          <motion.div
                            key={platform.id}
                            layout
                            initial={{ opacity: 0, y: 20, scale: 0.95 }}
                            animate={
                              isPageReady
                                ? { opacity: 1, y: 0, scale: 1 }
                                : { opacity: 0, y: 20, scale: 0.95 }
                            }
                            exit={{ opacity: 0, y: -20, scale: 0.95 }}
                            transition={{
                              duration: 0.4,
                              delay: isPageReady ? cardIndex * 0.08 + 0.2 : 0,
                              ease: [0.4, 0, 0.2, 1],
                            }}
                            whileHover={{ scale: 1.02, y: -4 }}
                            whileTap={{ scale: 0.98 }}
                            className={`
                    relative aspect-2/1 rounded-2xl overflow-hidden cursor-pointer group
                    glass
                    hover:shadow-xl transition-shadow
                    shrink-0 min-w-0 snap-start
                  `}
                            style={{
                              // Matches home 4x2 at all breakpoints (1 / sm:2 / lg:4); see REPORT_CARD_FLEX_BASIS
                              flexBasis: REPORT_CARD_FLEX_BASIS,
                              willChange: 'transform, opacity',
                            }} // 🚀 GPU加速
                            onClick={() => {
                              if (isPlayingOnStage) {
                                // 点击正在舞台播放的入口卡片：关闭舞台，恢复卡片内容
                                closeStageMode()
                                return
                              }
                              if (platformReport) {
                                openStageMode(platform.id)
                              } else if (isAdmin) {
                                generatePlatformReport(platform.id)
                              } else {
                                showToastMessage(
                                  t.reportsPage.adminOnlyGenerate,
                                  'warning',
                                )
                              }
                            }}
                          >
                          {/* 动态背景光效 */}
                          <div
                            className={`absolute -right-10 -top-10 w-40 h-40 bg-linear-to-br ${platform.color} opacity-10 rounded-full blur-3xl group-hover:opacity-20 transition-opacity`}
                          />

                          <div className="absolute inset-0 flex flex-col z-10">
                            {isLoading ? (
                              <div className="flex-1 flex items-center justify-center">
                                <PlatformReportGeneratingSpin
                                  className={platform.text}
                                />
                              </div>
                            ) : !platformReport ? (
                              <div className="flex-1 flex items-center justify-center">
                                <div className="text-center opacity-50 group-hover:opacity-80 transition-opacity">
                                  <div className="text-[10px] text-gray-400 font-medium">
                                    {isAdmin
                                      ? t.reportsPage.clickToGenerate
                                      : t.reportsPage.noReport}
                                  </div>
                                </div>
                              </div>
                            ) : (
                              <>
                                {/* 保持挂载：舞台占位时仅淡出，避免轮播计时器重置导致与其它卡片脱节 */}
                                <motion.div
                                  className="absolute inset-0"
                                  initial={false}
                                  animate={
                                    isPlayingOnStage
                                      ? { opacity: 0, scale: 0.98 }
                                      : { opacity: 1, scale: 1 }
                                  }
                                  transition={STAGE_PLACEHOLDER_TRANSITION}
                                  style={{
                                    pointerEvents: isPlayingOnStage
                                      ? 'none'
                                      : 'auto',
                                  }}
                                  aria-hidden={isPlayingOnStage}
                                >
                                  <ReportCardWidget
                                    config={platformWidgetConfigs[platform.id]}
                                    isEditMode={false}
                                    data={platformReport.card_visuals}
                                    bare
                                  />
                                </motion.div>
                                <AnimatePresence>
                                  {isPlayingOnStage && (
                                    <StagePlayingCardPlaceholder
                                      key="stage-playing"
                                      icon={platform.icon}
                                      name={platform.name}
                                      textClass={platform.text}
                                      borderClass={platform.border}
                                      label={t.reportsPage.stagePlaying}
                                    />
                                  )}
                                </AnimatePresence>
                              </>
                            )}
                          </div>

                          {/* 左下角平台标识：未生成/加载态也保留，避免丢失平台信息
                              （已生成态由 ReportCardWidget 自带的浮动 Logo 负责；
                               舞台播放占位态已在中心展示图标，此处不再重复） */}
                          {!platformReport && (
                            <div className="absolute bottom-3 left-3 z-20">
                              <div
                                className={`w-8 h-8 rounded-lg flex items-center justify-center text-base backdrop-blur-sm shadow-lg border ${platform.text} ${platform.bg} ${platform.border}`}
                              >
                                {platform.icon}
                              </div>
                            </div>
                          )}
                          </motion.div>
                        )
                      })}
                    </motion.div>
                  )}
                </>
              )}

              {activeTab === 'platform' &&
                platformVisibilityReady &&
                !hasEnabledPlatforms && (
                  <div className="pt-8 pb-12 -mt-7 -mb-11">
                    <motion.div
                      className="relative rounded-2xl overflow-hidden min-h-55"
                      initial={{ opacity: 0, y: 12 }}
                      animate={
                        isPageReady
                          ? { opacity: 1, y: 0 }
                          : { opacity: 0, y: 12 }
                      }
                      transition={{
                        duration: 0.3,
                        delay: isPageReady ? 0.15 : 0,
                      }}
                    >
                      <div className="absolute inset-0 bg-white/70 dark:bg-black/80 backdrop-blur-xl" />
                      <div
                        className="absolute -right-20 -top-20 w-48 h-48 rounded-full blur-3xl opacity-20"
                        style={{ background: 'var(--color-primary)' }}
                      />
                      <div
                        className="absolute -left-16 -bottom-16 w-32 h-32 rounded-full blur-2xl opacity-15"
                        style={{ background: 'var(--color-primary)' }}
                      />

                      <div className="relative z-10 h-full p-8 md:p-12 text-center flex flex-col items-center justify-center">
                        <div className="w-20 h-20 mx-auto mb-5 rounded-2xl bg-linear-to-br from-gray-100 to-gray-200 dark:from-white/10 dark:to-white/5 flex items-center justify-center shadow-lg">
                          <LuGlobe className="w-10 h-10 text-gray-400 dark:text-gray-500" />
                        </div>
                        <h3 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">
                          {t.reportsPage.noEnabledPlatforms}
                        </h3>
                        <p className="text-gray-500 dark:text-gray-400 text-sm max-w-sm mx-auto leading-6 line-clamp-2 min-h-12">
                          {t.reportsPage.noEnabledPlatformsDesc}
                        </p>
                      </div>

                      <div className="absolute inset-0 rounded-2xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
                    </motion.div>
                  </div>
                )}

              {activeTab === 'comprehensive' && (
                <>
                  {/* 综合报告提示条 */}
                  <motion.div
                    className={`h-12.5 ${isStageMode ? 'mb-2 md:mb-0' : ''}`}
                    initial={{ opacity: 0, x: -20 }}
                    animate={
                      isPageReady
                        ? { opacity: 1, x: 0 }
                        : { opacity: 0, x: -20 }
                    }
                    exit={{ opacity: 0, x: -20 }}
                    transition={{
                      duration: 0.3,
                      ease: 'easeOut',
                      delay: isPageReady ? 0.1 : 0,
                    }}
                  >
                    <motion.div
                      className="w-full md:w-[24%] h-full glass rounded-xl px-4 flex items-center gap-2 shadow-sm"
                      whileHover={{ scale: 1.02 }}
                      transition={{ duration: 0.2 }}
                    >
                      {isStageMode &&
                      stageReportData?.type === 'comprehensive' ? (
                        // 舞台模式下显示标题和控制按钮
                        <>
                          <FaMagic className="text-base shrink-0 text-accent-color" />
                          <div className="flex-1 min-w-0 flex items-center justify-between gap-2">
                            <span className="text-sm font-medium truncate">
                              {stageReportData.title}
                            </span>
                            <div className="flex items-center gap-2">
                              <button
                                onClick={() => {
                                  const event = new CustomEvent(
                                    'stage-toggle-pause',
                                  )
                                  window.dispatchEvent(event)
                                }}
                                className="w-8 h-8 rounded-lg bg-white dark:bg-neutral-800 hover:bg-gray-100 dark:hover:bg-neutral-600 flex items-center justify-center text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-all shadow-sm"
                                title={
                                  stagePaused
                                    ? t.reportsPage.continuePlay
                                    : t.reportsPage.pause
                                }
                              >
                                {stagePaused ? (
                                  <svg
                                    className="w-4 h-4"
                                    fill="currentColor"
                                    viewBox="0 0 24 24"
                                  >
                                    <path d="M8 5v14l11-7z" />
                                  </svg>
                                ) : (
                                  <svg
                                    className="w-4 h-4"
                                    fill="currentColor"
                                    viewBox="0 0 24 24"
                                  >
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
                                animate={
                                  anim.loop
                                    ? {
                                        rotate: [0, 10, -10, 10, 0],
                                        scale: [1, 1.1, 1.1, 1.1, 1],
                                      }
                                    : {}
                                }
                                transition={{
                                  duration: 2,
                                  repeat: anim.loop ? Infinity : 0,
                                  repeatDelay: 3,
                                  ease: 'easeInOut',
                                }}
                              >
                                <FaMagic className="text-base shrink-0 text-accent-color" />
                              </motion.div>
                              <div className="flex-1 min-w-0 flex items-center gap-2">
                                <input
                                  type="text"
                                  value={customStyle}
                                  onChange={(e) =>
                                    setCustomStyle(e.target.value)
                                  }
                                  placeholder={
                                    t.reportsPage.styleDescPlaceholder
                                  }
                                  className="clean-input flex-1 min-w-0 text-sm"
                                />
                                <motion.button
                                  onClick={generateComprehensiveReport}
                                  disabled={loadingPlatform === 'comprehensive'}
                                  className="shrink-0 h-6.5 px-3 text-white rounded text-xs font-medium transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
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
                                  {loadingPlatform === 'comprehensive' ? (
                                    <>
                                      <motion.div
                                        animate={{ rotate: 360 }}
                                        transition={{
                                          repeat: Infinity,
                                          duration: 1,
                                          ease: 'linear',
                                        }}
                                      >
                                        <FaSync size={10} />
                                      </motion.div>
                                      <span>{t.reportsPage.generating}</span>
                                    </>
                                  ) : (
                                    <>
                                      <motion.div
                                        animate={
                                          anim.loop
                                            ? {
                                                scale: [1, 1.2, 1],
                                                rotate: [0, 5, -5, 0],
                                              }
                                            : {}
                                        }
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
                              <FaMagic className="text-base shrink-0 opacity-50 text-accent-color" />
                              <div className="flex-1 min-w-0">
                                <div className="text-sm font-medium text-gray-600 dark:text-gray-400">
                                  {t.reportsPage.comprehensiveReport}
                                </div>
                                <div className="text-[10px] text-gray-500 dark:text-gray-500">
                                  {t.reportsPage.adminOnlyGenerateHint}
                                </div>
                              </div>
                            </>
                          )}
                        </>
                      )}
                    </motion.div>
                  </motion.div>
                  {displayedComprehensiveReports.length > 0 ? (
                    <motion.div
                      className={`${isStageMode ? 'hidden md:flex' : 'flex'} relative left-1/2 w-dvw max-w-none -translate-x-1/2 gap-4 overflow-x-auto scrollbar-hide snap-x snap-mandatory pt-8 pb-12 -mt-7 -mb-11 pr-3 xs:pr-4 sm:pr-6 md:pr-8 ${REPORT_CAROUSEL_CSS_VARS}`}
                      style={
                        {
                          paddingLeft:
                            'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.5rem)',
                          scrollPaddingLeft:
                            'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.5rem)',
                        } as React.CSSProperties
                      }
                      initial={{ opacity: 0 }}
                      animate={isPageReady ? { opacity: 1 } : { opacity: 0 }}
                      exit={{ opacity: 0 }}
                      transition={{
                        duration: 0.3,
                        delay: isPageReady ? 0.15 : 0,
                      }}
                    >
                      <AnimatePresence mode="popLayout">
                        {displayedComprehensiveReports.map(
                          (compReport: any, index: number) => (
                            <ComprehensiveReportCard
                              key={compReport.id}
                              compReport={compReport}
                              index={index}
                              onOpen={(
                                analysis: any,
                                _id: number,
                                _createdAt: string,
                              ) => {
                                // 打开综合报告的舞台模式
                                // 从所有平台报告中提取 library_items
                                const allLibraryItems: Array<{
                                  title: string
                                  cover?: string
                                  type: string
                                  platform?: string
                                }> = []

                                if (report?.platform_reports) {
                                  report.platform_reports.forEach(
                                    (platformReport) => {
                                      const cardVisuals: any =
                                        platformReport.card_visuals
                                      if (
                                        cardVisuals &&
                                        typeof cardVisuals === 'object'
                                      ) {
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
                                    },
                                  )
                                }

                                setStageReportData({
                                  type: 'comprehensive',
                                  综合分析: analysis,
                                  library_items: allLibraryItems,
                                  title:
                                    analysis.visual_style ||
                                    t.reportsPage.allPlatformReport,
                                })
                                setIsStageMode(true)
                              }}
                            />
                          ),
                        )}
                      </AnimatePresence>
                    </motion.div>
                  ) : (
                    <div className="pt-8 pb-12 -mt-7 -mb-11">
                      <EmptyComprehensiveReport
                        isAdmin={isAdmin}
                        isPageReady={isPageReady}
                      />
                    </div>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      </div>
    </AnimatedView>
  )
}
