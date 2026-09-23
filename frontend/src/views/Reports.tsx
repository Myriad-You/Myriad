import type { ReactNode } from 'react'
import type { ToastType } from '../components/Toast'
import type { WidgetConfig } from '../components/widgetGridTypes'
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
import AnimatedView from '../components/AnimatedView'
import { Spinner } from '../components/Spinner'
import { setStageLeaveHandler } from '../components/stageLeaveGate'
import StageMode from '../components/StageMode'
import { preloadPlatformFaces } from '../components/widgets/reportCard/platformFaceLoaders'
import { ReportCardWidget } from '../components/widgets/ReportCardWidget'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { usePageReady } from '../hooks/animation'
import { isExlight } from '../hooks/useAnimationLevel'
import { useHorizontalStripScroll } from '../hooks/useHorizontalStripScroll'
import { usePageSeo } from '../hooks/usePageSeo'
import {
  useResolvedTitleColor,
  useTitleFont,
} from '../hooks/useTitleFont'
import { ApiError, apiService } from '../services/api'
import {
  fetchPlatformData,
  generatePlatformReports,
} from '../services/platformTasksApi'
import { buildModulePageSeo } from '../utils/modulePageSeo'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../utils/moduleVisibility'
import { resolvePlatformId } from '../utils/platformId'
import { notifyRecentActivityUpdated } from '../utils/recentActivity'
import { REPORT_PLATFORM_IDS } from '../utils/reportCardVisuals'
import { reportUserFacingError } from '../utils/reportError'
import { getLatestReportDeduped, invalidateLatestReportCache } from '../utils/requestDedup'
import { showToast } from '../utils/toastManager'
import { userFacingError } from '../utils/userFacingError'
import { pickReportHook } from './reports/reportsDynamicStatus'
import ReportsStatusBar from './reports/ReportsStatusBar'
import {
  REPORT_CARD_FLEX_BASIS,
  REPORT_CAROUSEL_CSS_VARS,
  REPORT_STRIP_ALIGN_PAD,
} from './reports/types'

/** The readable reason a report endpoint put in `message`, if any. */
function bodyMessage(body: unknown): string | null {
  const message = (body as { message?: unknown } | null | undefined)?.message
  return typeof message === 'string' ? message : null
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

interface CrossPlatformReport {
  id?: number
  platform_reports: PlatformReport[]
  created_at: string
}

// Module-level so the array is not recreated.
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
    name: 'NetEase',
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
  // Eager-load platform faces (not React.lazy) so data can mount synchronously.
  useEffect(() => {
    void preloadPlatformFaces(REPORT_PLATFORM_IDS).catch(() => {})
  }, [])

  const { t, format } = useI18n()
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
  const { currentFont, titleFontSize } = useTitleFont()
  const titleColorPrimary = useResolvedTitleColor('primary')
  const platformStripScroll = useHorizontalStripScroll()

  const [loadingPlatform, setLoadingPlatform] = useState<string | null>(null)
  const [report, setReport] = useState<CrossPlatformReport | null>(null)
  const [isAdmin, setIsAdmin] = useState(false)
  const [enabledPlatformIds, setEnabledPlatformIds] = useState<string[]>([])
  const [platformVisibilityReady, setPlatformVisibilityReady] =
    useState(false)

  const [isStageMode, setIsStageMode] = useState(false)
  const [stagePaused, setStagePaused] = useState(false)
  const [refreshingStage, setRefreshingStage] = useState(false)
  const [playAllMode, setPlayAllMode] = useState(false)
  const [_playAllQueue, setPlayAllQueue] = useState<string[]>([])
  // Ref so play-all queue is not stale in closures.
  const playAllQueueRef = useRef<string[]>([])
  const [stageReportData, setStageReportData] = useState<{
    platform?: string
    summary?: string
    insights?: string[]
    card_visuals?: any
    type?: 'platform'
  } | null>(null)

  const showToastMessage = useCallback(
    (message: string, type: ToastType = 'info') => {
      showToast({ message, type, replaceKey: 'reports' })
    },
    [],
  )

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

  // Order follows backend platform_order.
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

  const platformReportsMap = useMemo(() => {
    const map = new Map<string, PlatformReport>()
    report?.platform_reports?.forEach((r) => map.set(r.platform, r))
    return map
  }, [report?.platform_reports])

  const reportHighlights = useMemo(() => {
    const items = []
    for (const platform of visiblePlatforms) {
      const report = platformReportsMap.get(platform.id)
      if (!report) continue
      const hook = pickReportHook(report)
      if (!hook) continue
      items.push({
        platformId: platform.id,
        platformName: platform.name,
        hook,
      })
    }
    return items
  }, [visiblePlatforms, platformReportsMap])

  // Stable config objects so ReportCardWidget memo holds.
  const platformWidgetConfigs = useMemo(() => {
    const map: Record<string, WidgetConfig> = {}
    PLATFORMS.forEach((p) => {
      map[p.id] = { config: { platformId: p.id } } as WidgetConfig
    })
    return map
  }, [])

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

  const closeStageMode = useCallback(() => {
    setIsStageMode(false)
    setPlayAllMode(false)
    setPlayAllQueue([])
    playAllQueueRef.current = []
    // Keep data through the exit animation.
  }, [])

  const handleUserCloseStage = useCallback(() => {
    closeStageMode()
  }, [closeStageMode])

  // 等内容淡出再放行路由（exlight 0，否则 520ms）。
  useEffect(() => {
    if (!isStageMode) {
      setStageLeaveHandler(null)
      return
    }
    setStageLeaveHandler((proceed) => {
      closeStageMode()
      window.setTimeout(proceed, isExlight() ? 0 : 520)
    })
    return () => setStageLeaveHandler(null)
  }, [isStageMode, closeStageMode])

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

  const startPlayAll = useCallback(() => {
    const platformsWithReports = Iterator.from(visiblePlatforms)
      .filter((p) => platformReportsMap.has(p.id))
      .map((p) => p.id)
      .toArray()

    if (platformsWithReports.length === 0) {
      showToastMessage(t.reportsPage.noPlatformReports, 'error')
      return
    }

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

  const playNextPlatform = useCallback(() => {
    if (playAllQueueRef.current.length === 0) {
      closeStageMode()
      showToastMessage(t.reportsPage.allPlaybackComplete, 'success')
      return
    }

    const nextPlatformId = playAllQueueRef.current[0]
    const remainingQueue = playAllQueueRef.current.slice(1)

    playAllQueueRef.current = remainingQueue
    setPlayAllQueue(remainingQueue)

    // Keep stage open; StageMode resets chapters from reportData.
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

  useEffect(() => {
    const handleStageComplete = () => {
      if (playAllMode && isStageMode) {
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

  // Merge one platform; do not replace the whole list.
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

  const refreshStageReport = useCallback(async () => {
    if (!stageReportData?.platform || stageReportData.type !== 'platform') {
      return
    }

    const platformId = stageReportData.platform
    const platformName =
      translatedPlatforms.find((p) => p.id === platformId)?.name || platformId

    setRefreshingStage(true)

    try {
      showToastMessage(
        format(t.reportsPage.refreshingReport, { platform: platformName }),
        'success',
      )

      try {
        const fetched = await fetchPlatformData(platformId)
        if (fetched.success !== false) notifyRecentActivityUpdated()
      } catch (fetchErr) {
        console.warn(`Refresh ${platformId} data request error:`, fetchErr)
      }

      // Any generation failure lands in the catch below as refreshReportFailed.
      const genBody = await generatePlatformReports<PlatformReport>([platformId])
      // Surface backend skip reasons (empty/unfetched).
      const skippedReason =
        genBody?.success === false
          ? genBody?.message
          : Array.isArray(genBody?.skipped)
            ? genBody.skipped.find((s: any) => s?.platform === platformId)
                ?.reason
            : null
      if (skippedReason) {
        showToastMessage(
          reportUserFacingError(
            skippedReason,
            t.reportsPage.generateFailed,
            t.reportsPage,
          ),
          'error',
        )
        return
      }

      // Merge the generate payload; do not refetch latest.
      const updatedPlatformReport: PlatformReport | undefined = Array.isArray(
        genBody?.reports,
      )
        ? genBody.reports.find((r: PlatformReport) => r.platform === platformId)
        : undefined

      if (updatedPlatformReport) {
        // Drop home-widget empty cache so own-page ReportCards re-fetch.
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
          format(t.reportsPage.reportRefreshSuccess, {
            platform: platformName,
          }),
          'success',
        )
      } else {
        showToastMessage(t.reportsPage.reportRefreshNoData, 'error')
      }
    } catch (err) {
      console.error('Refresh stage report failed:', err)
      showToastMessage(
        format(t.reportsPage.refreshReportFailed, { platform: platformName }),
        'error',
      )
    } finally {
      setRefreshingStage(false)
    }
  }, [stageReportData, mergePlatformReport, t.reportsPage, translatedPlatforms, showToastMessage, format])

  const {
    isAdmin: authIsAdmin,
    isAuthenticated,
    user,
  } = useAuth()

  useEffect(() => {
    let cancelled = false

    const fetchEnabledPlatforms = async () => {
      try {
        const data = await apiService.get<{ platforms?: unknown }>('/config/public')
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
          showToastMessage(
            userFacingError(err, t.errors.configFileReadFailed),
            'error',
          )
          setEnabledPlatformIds(PLATFORMS.map((platform) => platform.id))
          setPlatformVisibilityReady(true)
        }
      }
    }

    fetchEnabledPlatforms()

    return () => {
      cancelled = true
    }
  }, [showToastMessage, t.errors.configFileReadFailed])

  useEffect(() => {
    setIsAdmin(authIsAdmin)
  }, [authIsAdmin, isAuthenticated])

  useEffect(() => {
    const fetchLatestReport = async () => {
      try {
        // Shares the home report cards' cached read; any failure shows an empty set.
        const platformData = await getLatestReportDeduped().catch(() => null)
        let platformReports = []
        let createdAt = new Date().toISOString()
        if (platformData?.platform_reports) {
          platformReports = platformData.platform_reports
          createdAt = platformData.created_at || createdAt
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

  // Failures must toast; spinner-only is a silent fail.
  const generatePlatformReport = useCallback(
    async (platformId: string) => {
      setLoadingPlatform(platformId)
      const platformName =
        translatedPlatforms.find((p) => p.id === platformId)?.name || platformId
      try {
        const refreshFailed = format(t.reportsPage.refreshReportFailed, {
          platform: platformName,
        })
        let fetchWarning: string | null = null
        try {
          // Empty fetch returns success:false plus a readable reason.
          const fetched = await fetchPlatformData(platformId)
          if (fetched.success === false) {
            fetchWarning = reportUserFacingError(bodyMessage(fetched), refreshFailed, t.reportsPage)
            console.warn(fetchWarning)
          } else {
            notifyRecentActivityUpdated()
          }
        } catch (fetchErr) {
          fetchWarning = fetchErr instanceof ApiError && fetchErr.status > 0
            ? reportUserFacingError(bodyMessage(fetchErr.body), refreshFailed, t.reportsPage)
            : refreshFailed
          console.warn(`刷新 ${platformId} 数据请求出错:`, fetchErr)
        }

        let genBody
        try {
          genBody = await generatePlatformReports<PlatformReport>([platformId])
        } catch (genErr) {
          if (!(genErr instanceof ApiError) || genErr.status === 0) throw genErr
          throw new Error(
            reportUserFacingError(bodyMessage(genErr.body), t.reportsPage.generateFailed, t.reportsPage),
          )
        }

        if (genBody.success === false) {
          showToastMessage(
            reportUserFacingError(
              genBody.message || fetchWarning,
              t.reportsPage.generateFailed,
              t.reportsPage,
            ),
            'error',
          )
          return
        }
        const skippedReason = Array.isArray(genBody.skipped)
          ? genBody.skipped.find((s: any) => s?.platform === platformId)?.reason
          : null
        if (skippedReason) {
          showToastMessage(
            reportUserFacingError(
              skippedReason,
              t.reportsPage.generateFailed,
              t.reportsPage,
            ),
            'error',
          )
          return
        }

        const updated: PlatformReport | undefined = Array.isArray(
          genBody.reports,
        )
          ? genBody.reports.find((r: PlatformReport) => r.platform === platformId)
          : undefined
        if (updated) {
          invalidateLatestReportCache()
          mergePlatformReport(updated)
          // Fetch failed but cache generated: still warn.
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
          reportUserFacingError(
            err,
            t.reportsPage.generateFailedRetry,
            t.reportsPage,
          ),
          'error',
        )
      } finally {
        setLoadingPlatform(null)
      }
    },
    [t.reportsPage, translatedPlatforms, mergePlatformReport, showToastMessage, format],
  )

  return (
    <AnimatedView className="min-h-screen md:h-screen md:overflow-hidden">
      <StageMode
        isOpen={isStageMode}
        onClose={handleUserCloseStage}
        reportData={stageReportData}
        onRefresh={refreshStageReport}
        playAllMode={playAllMode}
      />
      <div className="flex flex-col pt-20 pb-24 md:pb-6 px-3 xs:px-4 sm:px-6 min-h-dvh md:h-dvh">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col gap-4 p-2">
          <div className="flex-1 md:flex-none md:h-[60%] rounded-2xl relative overflow-hidden" />

          <div
            className="md:h-[40%] flex flex-col relative justify-end md:justify-start"
            data-tour="reports-cards"
            data-tour-fit=".reports-platform-card, .reports-empty"
          >
            <div className="flex flex-col relative z-10">
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
                    stagePlatformHero={
                      stageReportData?.platform === 'netease'
                        ? 'NetEase'
                        : PLATFORMS.find(
                            (p) => p.id === stageReportData?.platform,
                          )?.name ||
                          stageReportData?.platform ||
                          null
                    }
                    enabledPlatformCount={visiblePlatforms.length}
                    reportCount={visiblePlatforms.filter((p) =>
                      platformReportsMap.has(p.id),
                    ).length}
                    hasEnabledPlatforms={hasEnabledPlatforms}
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
                    highlights={reportHighlights}
                    stageCompactHero={isStageMode}
                    copy={{
                      heroStage: t.reportsPage.heroStage,
                      platformReport: t.reportsPage.platformReport,
                      noEnabledPlatforms: t.reportsPage.noEnabledPlatforms,
                      stagePlaying: t.reportsPage.stagePlaying,
                      stagePaused: t.reportsPage.stagePaused,
                      tipNoReports: t.reportsPage.tipNoReports,
                      tipNoReportsSub: t.reportsPage.tipNoReportsSub,
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
                      top: `calc(30px - ${7.5 * titleFontSize}rem)`,
                      fontFamily: currentFont.family,
                      fontSize: `${6 * titleFontSize}rem`,
                      color: titleColorPrimary,
                      webkitTextStroke: `0.5px color-mix(in srgb, ${titleColorPrimary} 30%, transparent)`,
                    }}
                    onPlayAll={startPlayAll}
                    onRefreshStage={refreshStageReport}
                    onCloseStage={closeStageMode}
                  />
                  {hasEnabledPlatforms && (
                    <motion.div
                      ref={platformStripScroll.ref}
                      className={`${isStageMode ? 'hidden md:flex' : 'flex'} relative left-1/2 w-dvw max-w-none -translate-x-1/2 gap-4 overflow-x-auto scrollbar-hide snap-x snap-mandatory touch-pan-x pt-8 pb-12 -mt-7 -mb-11 pr-3 xs:pr-4 sm:pr-6 md:pr-8 ${REPORT_CAROUSEL_CSS_VARS} ${platformStripScroll.className}`}
                      style={
                        {
                          paddingLeft: REPORT_STRIP_ALIGN_PAD,
                          scrollPaddingLeft: REPORT_STRIP_ALIGN_PAD,
                          ...platformStripScroll.style,
                        } as React.CSSProperties
                      }
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
                        const platformReport = platformReportsMap.get(platform.id)
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
                    reports-platform-card
                    relative aspect-2/1 rounded-2xl overflow-hidden cursor-pointer group
                    glass
                    hover:shadow-xl transition-shadow
                    shrink-0 min-w-0 snap-start
                  `}
                            style={{
                              flexBasis: REPORT_CARD_FLEX_BASIS,
                              willChange: 'transform, opacity',
                            }}
                            onClick={() => {
                              if (isPlayingOnStage) {
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
                                {/* Stay mounted during stage placeholder so carousel timers do not reset. */}
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

                          {/* Keep the corner mark while empty/loading; generated cards use ReportCardWidget's logo. */}
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
                  <div className="pt-8 pb-12 -mt-7 -mb-11 px-1">
                    <motion.div
                      className="reports-empty relative rounded-2xl overflow-hidden min-h-55"
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
    </AnimatedView>
  )
}
