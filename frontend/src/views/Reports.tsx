import type { ReactNode } from 'react'
import type { ToastType } from '../components/Toast'
import type { WidgetConfig } from '../components/WidgetGrid'
import type { LifeStatusKind } from './reports/reportsDynamicStatus'

import {
  FaGithub,
  FaSteam,
  FaXbox,
  FaXTwitter,
  LuGlobe,
  SiBangumi,
  SiBilibili,
  SiDiscord,
  SiMyanimelist,
  SiNeteasecloudmusic,
  SiPlaystation,
  SiYoutube,
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
import { useNavigate } from 'react-router-dom'
import PersonaOnboarding from '../components/agent/PersonaOnboarding'
import AnimatedView from '../components/AnimatedView'
import { Spinner } from '../components/Spinner'
import StageMode from '../components/StageMode'
import Toast from '../components/Toast'
import { preloadPlatformFaces } from '../components/widgets/reportCard/platformFaceLoaders'
import { ReportCardWidget } from '../components/widgets/ReportCardWidget'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import {
  usePageReady,
  useReportsScheduler,
} from '../hooks/animation'
import { useHorizontalStripScroll } from '../hooks/useHorizontalStripScroll'
import { usePageSeo } from '../hooks/usePageSeo'
import {
  useResolvedTitleColor,
  useTitleFont,
} from '../hooks/useTitleFont'
import { agentService } from '../services/agent'
import { getCSRFToken } from '../utils/csrf'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'
import { buildModulePageSeo } from '../utils/modulePageSeo'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../utils/moduleVisibility'
import { resolvePlatformId } from '../utils/platformId'
import { notifyRecentActivityUpdated } from '../utils/recentActivity'
import { REPORT_PLATFORM_IDS } from '../utils/reportCardVisuals'
import { invalidateLatestReportCache } from '../utils/requestDedup'
import { hasSessionHint } from '../utils/sessionDetection'
import ReportsStatusBar from './reports/ReportsStatusBar'
import {
  REPORT_CARD_FLEX_BASIS,
  REPORT_CAROUSEL_CSS_VARS,
} from './reports/types'

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

interface CrossPlatformReport {
  id?: number // 报告ID
  platform_reports: PlatformReport[]
  created_at: string
}

// 平台配置常量（已在组件外部，避免重复创建）
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
    id: 'youtube',
    name: 'YouTube',
    icon: <SiYoutube />,
    color: 'from-red-500 to-red-700',
    bg: 'bg-red-50/10 dark:bg-red-900/10',
    text: 'text-red-600 dark:text-red-400',
    border: 'border-red-200/20 dark:border-red-800/20',
    widgetType: 'video',
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

function PlatformReportGeneratingSpin({
  className = '',
}: {
  className?: string
}) {
  return <Spinner size="sm" className={className} />
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

export default function Reports() {
  // 🆕 初始化报告页调度器（Visibility + Interval + RAF + DOMBatch）
  useReportsScheduler()

  // 进页即预热全部平台 face（非 React.lazy）：数据到达时可同步挂载，保住入场
  useEffect(() => {
    void preloadPlatformFaces(REPORT_PLATFORM_IDS).catch(() => {})
  }, [])

  const { t } = useI18n()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.reports,
    { isAuthenticated: false, isAdmin: false },
  )

  usePageSeo(
    useMemo(
      () =>
        buildModulePageSeo({
          label: t.reports.title || t.nav.reports,
          description: t.widgets.dualLayerAnalysis,
          path: '/reports',
          moduleOpenToAll,
        }),
      [t, moduleOpenToAll],
    ),
  )

  const isPageReady = usePageReady()
  // 🆕 标题字体 Hook
  const { currentFont, titleFontSize } = useTitleFont()
  // 自适应色对齐 Tapp 音乐播放器歌词：对比度推导
  const titleColorPrimary = useResolvedTitleColor('primary')
  // 平台报告卡片条：滚轮横向滚动 + 鼠标拖拽
  const platformStripScroll = useHorizontalStripScroll()

  const [loadingPlatform, setLoadingPlatform] = useState<string | null>(null)
  const [report, setReport] = useState<CrossPlatformReport | null>(null)
  const [isAdmin, setIsAdmin] = useState(false)
  const [enabledPlatformIds, setEnabledPlatformIds] = useState<string[]>([])
  const [platformVisibilityReady, setPlatformVisibilityReady] =
    useState(false)

  // 舞台模式状态
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
    type?: 'platform'
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

  // 监听舞台暂停状态
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

  // 缓存平台报告映射，避免重复查找
  const platformReportsMap = useMemo(() => {
    const map = new Map<string, PlatformReport>()
    report?.platform_reports?.forEach((r) => map.set(r.platform, r))
    return map
  }, [report?.platform_reports])

  // 稳定的 ReportCardWidget config，避免每次渲染新建对象打破 memo
  const platformWidgetConfigs = useMemo(() => {
    const map: Record<string, WidgetConfig> = {}
    PLATFORMS.forEach((p) => {
      map[p.id] = { config: { platformId: p.id } } as WidgetConfig
    })
    return map
  }, [])

  // 打开舞台模式
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
        void import('../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.REPORT_STAGE_OPEN, {
              target: platformId,
              throttleMs: 3000,
            })
          },
        )
      }
    },
    [platformReportsMap],
  )

  // 关闭舞台模式
  const closeStageMode = useCallback(() => {
    setIsStageMode(false)
    setPlayAllMode(false)
    setPlayAllQueue([])
    playAllQueueRef.current = []
    // 不立即清除数据，以便播放退出动画
    // setStageReportData(null);
  }, [])

  // 用户手动关闭舞台（无论什么模式都完全退出）
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

  // 开始播放所有平台
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
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.REPORT_PLAY_ALL, {
          target: firstPlatformId,
          throttleMs: 5000,
        })
      },
    )

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

  // 播放下一个平台（播放所有模式）
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
        created_at: updated.created_at || prev?.created_at || new Date().toISOString(),
      }
    })
  }, [])

  // 刷新当前舞台模式的平台报告
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
        notifyHttpRateLimit(fetchResponse)
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

      notifyHttpRateLimit(response)
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
    user,
  } = useAuth()
  const navigate = useNavigate()
  const [lifeKind, setLifeKind] = useState<LifeStatusKind>('loading')
  const [lifeName, setLifeName] = useState('Arael')
  const [lifeMood, setLifeMood] = useState(70)
  const [lifeActivity, setLifeActivity] = useState('idle')
  const [lifeEnabled, setLifeEnabled] = useState<boolean | null>(null)
  const [showPersonaOnboarding, setShowPersonaOnboarding] = useState(false)

  const refreshLifeEntry = useCallback(async () => {
    // 开关未知时先说「正在确认」：否则关着生命的站点会让游客先看到登录提示，
    // 等公共配置回来再跳成「已关闭」。
    if (lifeEnabled === null) {
      setLifeKind('loading')
      return
    }
    if (!lifeEnabled) {
      setLifeKind('disabled')
      return
    }
    if (!isAuthenticated) {
      setLifeKind('guest')
      return
    }
    setLifeKind('loading')
    try {
      const persona = await agentService.getPersona()
      setLifeName(persona?.name?.trim() || 'Arael')
      setLifeMood(
        typeof persona?.mood === 'number' && Number.isFinite(persona.mood)
          ? persona.mood
          : 70,
      )
      setLifeActivity(persona?.activity?.trim() || 'idle')
      if (persona?.hasCustomPersona) {
        setLifeKind('ready')
      } else if (user?.is_owner) {
        setLifeKind('create')
      } else {
        setLifeKind('ready')
      }
    } catch {
      setLifeName('Arael')
      setLifeKind(user?.is_owner ? 'create' : 'ready')
    }
  }, [isAuthenticated, lifeEnabled, user?.is_owner])

  useEffect(() => {
    void refreshLifeEntry()
  }, [refreshLifeEntry])

  useEffect(() => {
    const onPersonaUpdated = () => {
      void refreshLifeEntry()
    }
    window.addEventListener('arael-persona-updated', onPersonaUpdated)
    return () =>
      window.removeEventListener('arael-persona-updated', onPersonaUpdated)
  }, [refreshLifeEntry])

  const seedTags = useMemo(() => {
    const tags: string[] = []
    for (const report of platformReportsMap.values()) {
      for (const insight of report.insights || []) {
        const phrase = insight.replace(/\s+/g, ' ').trim()
        if (phrase.length >= 2 && phrase.length <= 40) tags.push(phrase)
      }
    }
    return tags
  }, [platformReportsMap])

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
          if (typeof data.agentLifeEnabled === 'boolean') {
            setLifeEnabled(data.agentLifeEnabled)
          }
          if (
            typeof data.agentPersonaName === 'string' &&
            data.agentPersonaName.trim()
          ) {
            setLifeName(data.agentPersonaName.trim())
          }
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
    if (lifeEnabled !== null || !isAuthenticated) return
    let cancelled = false
    void (async () => {
      try {
        const persona = await agentService.getPersona()
        if (!cancelled) setLifeEnabled(persona !== null)
      } catch {
        // Keep unknown — don't pretend the life switch is off.
      }
    })()
    return () => {
      cancelled = true
    }
  }, [lifeEnabled, isAuthenticated])

  useEffect(() => {
    setIsAdmin(authIsAdmin)
  }, [authIsAdmin, isAuthenticated])

  // 加载最新平台报告
  useEffect(() => {
    const fetchLatestReport = async () => {
      try {
        const platformResponse = await fetch(`${API_URL}/api/reports/latest`, {
          credentials: 'include',
        })

        let platformReports = []
        let createdAt = new Date().toISOString()

        if (platformResponse.ok) {
          const platformData = await platformResponse.json()
          if (platformData.platform_reports) {
            platformReports = platformData.platform_reports
            createdAt = platformData.created_at || createdAt
          }
        }

        setReport({
          platform_reports: platformReports,
          created_at: createdAt,
        })
      } catch (err) {
        console.error('获取最新报告失败:', err)
      }
    }

    fetchLatestReport()
  }, [])

  // 生成单个平台报告 - 性能优化：使用 useCallback
  // 失败路径必须 toast（勿只 console）：用户点「点击生成」后 spinner 消失且无反馈即「静默失败」
  const generatePlatformReport = useCallback(
    async (platformId: string) => {
      setLoadingPlatform(platformId)
      const platformName =
        translatedPlatforms.find((p) => p.id === platformId)?.name || platformId
      try {
        const csrfToken = await getCSRFToken(true)
        if (!csrfToken) {
          showToastMessage(t.reportsPage.getTokenFailed, 'error')
          return
        }

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
          notifyHttpRateLimit(fetchResponse)

          // 后端现在会在数据为空时返回 success:false + 可读原因
          const fetchBody = await fetchResponse.json().catch(() => null)
          if (!fetchResponse.ok || fetchBody?.success === false) {
            fetchWarning =
              (typeof fetchBody?.message === 'string' && fetchBody.message) ||
              t.reportsPage.refreshReportFailed.replace(
                '{platform}',
                platformName,
              )
            console.warn(fetchWarning)
          } else {
            notifyRecentActivityUpdated()
          }
        } catch (fetchErr) {
          fetchWarning = t.reportsPage.refreshReportFailed.replace(
            '{platform}',
            platformName,
          )
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
        notifyHttpRateLimit(response)

        const genBody = await response.json().catch(() => null)
        if (!response.ok) {
          const msg =
            (typeof genBody?.message === 'string' && genBody.message) ||
            t.reportsPage.generateFailed
          throw new Error(msg)
        }

        if (!genBody) {
          showToastMessage(t.reportsPage.generateFailed, 'error')
          return
        }

        // 解析生成结果，把后端给出的跳过原因透出给用户
        if (genBody.success === false) {
          showToastMessage(
            genBody.message || fetchWarning || t.reportsPage.generateFailed,
            'error',
          )
          return
        }
        const skippedReason = Array.isArray(genBody.skipped)
          ? genBody.skipped.find((s: any) => s?.platform === platformId)?.reason
          : null
        if (skippedReason) {
          showToastMessage(String(skippedReason), 'error')
          return
        }

        // 3. 局部 merge 生成结果，避免整表替换其它平台报告
        const updated: PlatformReport | undefined = Array.isArray(
          genBody.reports,
        )
          ? genBody.reports.find((r: PlatformReport) => r.platform === platformId)
          : undefined
        if (updated) {
          invalidateLatestReportCache()
          mergePlatformReport(updated)
          // 抓取失败但用缓存生成成功时，仍提示抓取问题，避免「半失败」无感
          if (fetchWarning) {
            showToastMessage(fetchWarning, 'warning')
          }
        } else {
          showToastMessage(
            fetchWarning || t.reportsPage.reportRefreshNoData,
            'error',
          )
        }
      } catch (err) {
        console.error('Generate platform report failed:', err)
        showToastMessage(
          err instanceof Error && err.message
            ? err.message
            : t.reportsPage.generateFailedRetry,
          'error',
        )
      } finally {
        setLoadingPlatform(null)
      }
    },
    [t.reportsPage, translatedPlatforms, mergePlatformReport, showToastMessage],
  )

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
            <div className="flex flex-col gap-3 relative z-10">
              {platformVisibilityReady && (
                <>
                  <ReportsStatusBar
                    isPageReady={isPageReady}
                    isStageMode={isStageMode}
                    stagePaused={stagePaused}
                    stagePlatformId={stageReportData?.platform}
                    stagePlatformName={
                      stageReportData?.platform
                        ? translatedPlatforms.find(
                            (p) => p.id === stageReportData.platform,
                          )?.name || stageReportData.platform
                        : null
                    }
                    stagePlatformHero={stageReportData?.platform || null}
                    enabledPlatformCount={visiblePlatforms.length}
                    reportCount={visiblePlatforms.filter((p) =>
                      platformReportsMap.has(p.id),
                    ).length}
                    hasEnabledPlatforms={hasEnabledPlatforms}
                    platforms={visiblePlatforms}
                    showLife
                    lifeKind={lifeKind}
                    lifeSnapshot={{
                      name: lifeName,
                      mood: lifeMood,
                      activity: lifeActivity,
                    }}
                    isAdmin={isAdmin}
                    refreshingStage={refreshingStage}
                    viewer={
                      isAuthenticated
                        ? {
                            name: user?.display_name || user?.username,
                            avatarUrl: user?.avatar_url,
                          }
                        : null
                    }
                    stageCompactHero={isStageMode}
                    copy={{
                      heroStage: t.reportsPage.heroStage,
                      heroLife: t.reportsPage.heroLife,
                      platformReport: t.reportsPage.platformReport,
                      clickToView: t.reportsPage.clickToView,
                      noEnabledPlatforms: t.reportsPage.noEnabledPlatforms,
                      stagePlaying: t.reportsPage.stagePlaying,
                      stagePaused: t.reportsPage.stagePaused,
                      tipPlatformCount: t.reportsPage.tipPlatformCount,
                      tipPlatformCountSub: t.reportsPage.tipPlatformCountSub,
                      tipReportReady: t.reportsPage.tipReportReady,
                      tipReportReadySub: t.reportsPage.tipReportReadySub,
                      tipNoReports: t.reportsPage.tipNoReports,
                      tipNoReportsSub: t.reportsPage.tipNoReportsSub,
                      lifeTitle: t.arael.statusBarTitle,
                      lifeLoading: t.arael.statusLoading,
                      lifeDisabled: t.arael.statusDisabled,
                      lifeNeedLogin: t.arael.statusNeedLogin,
                      lifeCreateHint: t.arael.statusCreateHint,
                      lifeReadyHint: t.arael.statusReadyHint,
                      lifeIdle: t.arael.statusIdle,
                      lifeThinking: t.arael.statusThinking,
                      lifeTalking: t.arael.statusTalking,
                    }}
                    lifeActionLabels={{
                      create: t.arael.statusCreate,
                      open: t.arael.statusOpen,
                      login: t.arael.statusLogin,
                    }}
                    actionTitles={{
                      playAll: t.reportsPage.playAllReports,
                      refreshing: t.reportsPage.refreshing,
                      refreshCurrent: t.reportsPage.refreshCurrentReport,
                      continuePlay: t.reportsPage.continuePlay,
                      pause: t.reportsPage.pause,
                      closeStage: t.reportsPage.closeStage,
                    }}
                    titleStyle={{
                      top: `calc(29px - ${7.5 * titleFontSize}rem)`,
                      fontFamily: currentFont.family,
                      fontSize: `${6 * titleFontSize}rem`,
                      color: titleColorPrimary,
                      webkitTextStroke: `0.5px color-mix(in srgb, ${titleColorPrimary} 30%, transparent)`,
                    }}
                    onPlayAll={startPlayAll}
                    onRefreshStage={refreshStageReport}
                    onCloseStage={closeStageMode}
                    onLogin={() => navigate('/login')}
                    onOpenLife={() => {
                      if (lifeKind === 'create') {
                        setShowPersonaOnboarding(true)
                        return
                      }
                      window.dispatchEvent(
                        new CustomEvent('arael-open-session', {
                          detail: { sessionId: '' },
                        }),
                      )
                    }}
                  />
                  {hasEnabledPlatforms && (
                    <motion.div
                      ref={platformStripScroll.ref}
                      className={`${isStageMode ? 'hidden md:flex' : 'flex'} relative left-1/2 w-dvw max-w-none -translate-x-1/2 gap-4 overflow-x-auto scrollbar-hide snap-x snap-mandatory touch-pan-x pt-8 pb-12 -mt-7 -mb-11 pr-3 xs:pr-4 sm:pr-6 md:pr-8 ${REPORT_CAROUSEL_CSS_VARS} ${platformStripScroll.className}`}
                      style={
                        {
                          paddingLeft:
                            'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.5rem)',
                          scrollPaddingLeft:
                            'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.5rem)',
                          ...platformStripScroll.style,
                        } as React.CSSProperties
                      }
                      onWheel={platformStripScroll.onWheel}
                      onPointerDown={platformStripScroll.onPointerDown}
                      onPointerMove={platformStripScroll.onPointerMove}
                      onPointerUp={platformStripScroll.onPointerUp}
                      onPointerCancel={platformStripScroll.onPointerCancel}
                      onClickCapture={platformStripScroll.onClickCapture}
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
                        // 使用 Map 查找，O(1) 复杂度
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
                            }} // GPU加速
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
                                className={`w-8 h-8 rounded-lg flex items-center justify-center text-base shadow-lg border ${platform.text} ${platform.bg} ${platform.border}`}
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

              {platformVisibilityReady && !hasEnabledPlatforms && (
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
                      <div className="absolute inset-0 glass-surface glass-70" />
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
            </div>
          </div>
        </div>
      </div>
      <PersonaOnboarding
        open={showPersonaOnboarding}
        seedTags={seedTags}
        onClose={() => setShowPersonaOnboarding(false)}
        onSaved={(name) => {
          setLifeName(name)
          setLifeKind('ready')
          window.dispatchEvent(
            new CustomEvent('arael-open-session', {
              detail: { sessionId: '' },
            }),
          )
        }}
      />
    </AnimatedView>
  )
}
