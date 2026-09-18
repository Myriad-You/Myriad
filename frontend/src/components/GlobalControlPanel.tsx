import type { DynamicContentType } from '../services/DynamicContentProvider'

import type { AppNotification } from '../services/notificationApi'
import type { QuoteData, WeatherData } from '../utils/dynamicContent'
import type { PanelTab } from './ControlPanel/panelTransition'
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'

import { useNavigate } from 'react-router-dom'
import { useAnimationPreference } from '../contexts/AnimationPreferenceContext'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { resetMeropeState } from '../features/merope/meropeAffectState'
import { setForegroundSurface } from '../features/merope/perception/surface'
import { batchRead, batchWrite, observeResize } from '../hooks/animation'
import {
  isReducedAnimation,
  useAnimationLevel,
} from '../hooks/useAnimationLevel'
import { useMusicPlayer } from '../hooks/useMusicPlayer'
import { useNotificationCenter } from '../hooks/useNotificationCenter'
import { useNotificationPreferences } from '../hooks/useNotificationPreferences'
import { usePerformanceProfile } from '../hooks/usePerformanceProfile'
import { useWallpaper } from '../hooks/useWallpaper'
import { getDynamicContentProvider } from '../services/DynamicContentProvider'
import {
  notificationSourceFor,
  notificationToastType,
  shouldEmitNotificationToast,
  shouldSurfaceNotification,
} from '../services/notificationDelivery'
import { useBackgroundResidents } from '../tapp/runtime/backgroundResidentStore'
import {
  getGreeting,
  getRandomQuote,
  getWeatherInfo,
  WEATHER_ICON_ASSETS,
} from '../utils/dynamicContent'
import {
  allowsIslandType,
  DEFAULT_ISLAND_CONTENT,
  ISLAND_CONTENT_CHANGED_EVENT,
  islandContentFromPublicUi,
} from '../utils/islandContent'
import { CONTROL_PANEL_HEIGHT_COMPENSATION } from '../utils/libraryDockStage'
import { formatMusicError } from '../utils/musicError'
import {
  getNavLayoutSnapshot,
  getServerNavLayoutSnapshot,
  subscribeNavLayout,
} from '../utils/navLayout'
import {
  notificationFacingBody,
  notificationFacingTitle,
} from '../utils/notificationFacing'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { loadResource } from '../utils/resourceLoader'
import { useThemeMode } from '../utils/themeSubscriber'
import { showToast } from '../utils/toastManager'
import {
  isLookingAtAgentPanel,
  subscribeLookingAtAgentPanel,
} from './agent-panel/agentPanelVisible'
import { ADDRESSEE_UPDATED_EVENT } from './agent/meropeVitals'
import { ControlQuickActions } from './ControlPanel/ControlQuickActions'
import { islandPanelTabForClick } from './ControlPanel/islandClick'
import {
  initialPanelState,
  isPanelMorphing,
  isPanelOpen,
  mountsNotifications,
  panelReducer,
  resolvePanelMotion,
  settleTimeoutMs,
  showsDynamicContent,
  showsOverlay,
  showsPanelContent,
  showsProgressUi,
} from './ControlPanel/panelTransition'
import { ControlPanelWidgets } from './ControlPanel/ControlPanelWidgets'
import { MusicPlayer } from './ControlPanel/MusicPlayer'
import { UserSection } from './ControlPanel/UserSection'
import { isHoverCapablePointer } from './ControlPanel/widgetCarousel'
import {
  NotificationSourceIcon,
  notificationSourceIconAsset,
} from './notifications/NotificationIcons'
import { homeBrowseTourPanelPose } from './tour/tourHomePose'
import {
  getTourSnapshot,
  subscribeTour,
} from './tour/tourStore'
import NotificationPanelList from './NotificationPanelList'
import { WeatherAssetIcon } from './weather/WeatherAssetIcon'
import './GlobalControlPanel.css'

function readHomeBrowseTourPanelPose() {
  const snapshot = getTourSnapshot()
  return homeBrowseTourPanelPose(snapshot.tourId, snapshot.step?.id ?? null)
}

const GREETING_ICON_ASSETS = {
  sunrise: '/icons/greeting/sunrise.webp',
  sun: WEATHER_ICON_ASSETS.sunny,
  cloudSun: WEATHER_ICON_ASSETS.partlyCloudy,
  sunset: '/icons/greeting/sunset.webp',
  moon: '/icons/greeting/night.webp',
} as const

const DYNAMIC_ICON_ASSETS = {
  quote: '/icons/dynamic/quote.webp',
  music: '/icons/dynamic/music.webp',
  musicPaused: '/icons/dynamic/music-paused.webp',
} as const

type ThemePreference = 'light' | 'dark' | 'auto'

const THEME_CYCLE: ThemePreference[] = ['light', 'dark', 'auto']

function getStoredThemePreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'auto'
  const stored = localStorage.getItem('theme')
  return stored === 'light' || stored === 'dark' ? stored : 'auto'
}

function applyThemeClass(dark: boolean) {
  const html = document.documentElement
  if (dark) {
    html.classList.add('dark')
    html.classList.remove('light')
  } else {
    html.classList.add('light')
    html.classList.remove('dark')
  }

  const metaThemeColor = document.querySelector('meta[name="theme-color"]')
  if (metaThemeColor) {
    const primaryColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#94a3b8'
    metaThemeColor.setAttribute('content', primaryColor)
  }
}

interface DynamicContent {
  type: DynamicContentType
  icon: React.ReactNode
  text: string
  subtext?: string
  showSubtext?: boolean
  sourceTappId?: string
  lyricDuration?: number
}

const GlobalControlPanel: React.FC = () => {
  const navigate = useNavigate()
  const { user } = useAuth()
  useLayoutEffect(() => {
    resetMeropeState()
  }, [user?.id])
  const { locale, setLocale, t, format } = useI18n()
  const backgroundResidents = useBackgroundResidents()
  const navLayout = useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  const lookingAtAgent = useSyncExternalStore(
    subscribeLookingAtAgentPanel,
    isLookingAtAgentPanel,
    () => false,
  )
  // 触控带不挂天气/一言格，只留播放器与设置。
  const showControlPanelWidgets = navLayout === 'desktop'
  const { preferences: notificationPreferences } = useNotificationPreferences(
    user?.id,
  )
  // 展开/收起唯一状态：phase（issue #320）。
  const [panel, dispatchPanel] = useReducer(panelReducer, initialPanelState)
  const isExpanded = isPanelOpen(panel)
  const showDynamicContent = showsDynamicContent(panel)
  const showPanelContent = showsPanelContent(panel)
  const showOverlay = showsOverlay(panel)
  const panelTab = panel.tab
  const isDark = useThemeMode()

  const [isPageVisible, setIsPageVisible] = useState(!document.hidden)
  const pendingUpdatesRef = useRef<
    Array<(prev: DynamicContent[]) => DynamicContent[]>
  >([])

  useEffect(() => {
    const handleVisibilityChange = () => {
      const visible = !document.hidden
      setIsPageVisible(visible)
    }
    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [])

  const [dynamicContents, setDynamicContents] = useState<DynamicContent[]>([])
  const [currentContentIndex, setCurrentContentIndex] = useState(0)
  const [isHovering, setIsHovering] = useState(false)
  const [isTransitioning, setIsTransitioning] = useState(false)
  const [weatherData, setWeatherData] = useState<WeatherData | null>(null)
  const [quoteData, setQuoteData] = useState<QuoteData | null>(null)
  const [islandContent, setIslandContent] = useState(DEFAULT_ISLAND_CONTENT)

  const safeSetDynamicContents = useCallback(
    (updater: (prev: DynamicContent[]) => DynamicContent[]) => {
      if (document.hidden) {
        pendingUpdatesRef.current.push(updater)
      } else {
        setDynamicContents(updater)
      }
    },
    [],
  )

  useEffect(() => {
    if (isPageVisible && pendingUpdatesRef.current.length > 0) {
      // 先截取队列再 setState：updater 不在调用点同步跑，先清空会丢后台累积。
      const pendingUpdates = pendingUpdatesRef.current
      pendingUpdatesRef.current = []

      setDynamicContents((prev) => {
        let result = prev
        for (const updater of pendingUpdates) {
          result = updater(result)
        }
        return result
      })
    }
  }, [isPageVisible])

  const notifCarouselTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )

  useEffect(
    () => () => {
      if (notifCarouselTimerRef.current) {
        clearTimeout(notifCarouselTimerRef.current)
      }
    },
    [],
  )

  const handleNewNotification = useCallback(
    (n: AppNotification) => {
      const source = notificationSourceFor(n)
      const icon = (
        <NotificationSourceIcon source={source} className="h-4 w-4" />
      )
      const title = notificationFacingTitle(n)
      const body = notificationFacingBody(n)
      const snippet = body.length > 60 ? `${body.slice(0, 60)}…` : body

      if (
        shouldSurfaceNotification(
          notificationPreferences,
          n,
          'island',
          lookingAtAgent,
        )
      ) {
        safeSetDynamicContents((prev) => [
          {
            type: 'notification',
            icon,
            text: title,
            subtext: snippet,
            showSubtext: true,
          },
          ...prev.filter((c) => c.type !== 'notification'),
        ])
        setCurrentContentIndex(0)
        if (notifCarouselTimerRef.current) {
          clearTimeout(notifCarouselTimerRef.current)
        }
        notifCarouselTimerRef.current = setTimeout(() => {
          safeSetDynamicContents((prev) =>
            prev.filter((c) => c.type !== 'notification'),
          )
        }, 20000)
      }

      // Toast 只改视觉类型，不决定是否展示。
      if (
        shouldEmitNotificationToast(
          notificationPreferences,
          n,
          lookingAtAgent,
        )
      ) {
        const showInPanel = shouldSurfaceNotification(
          notificationPreferences,
          n,
          'panel',
          lookingAtAgent,
        )
        showToast({
          title,
          message: snippet,
          type: notificationToastType(n),
          icon: notificationSourceIconAsset(source),
          duration: 6000,
          showCloseButton: true,
          onClick: showInPanel
            ? () => {
                // 复用打开面板事件带 tab：已展开则只切 tab。
                window.dispatchEvent(
                  new CustomEvent('open-control-panel', {
                    detail: { tab: 'notifications' },
                  }),
                )
              }
            : undefined,
        })
      }

      if (
        document.hidden &&
        shouldSurfaceNotification(
          notificationPreferences,
          n,
          'browser',
          lookingAtAgent,
        ) &&
        typeof Notification !== 'undefined' &&
        Notification.permission === 'granted'
      ) {
        try {
          // Notification 构造即展示；同 id 用 tag 去重。
          void new Notification(title, {
            body: body.slice(0, 200),
            tag: n.id,
            icon: notificationSourceIconAsset(source),
          })
        } catch {
        }
      }
    },
    [lookingAtAgent, notificationPreferences, safeSetDynamicContents],
  )

  const includeNotificationInPanel = useCallback(
    (notification: AppNotification) =>
      shouldSurfaceNotification(
        notificationPreferences,
        notification,
        'panel',
        lookingAtAgent,
      ),
    [lookingAtAgent, notificationPreferences],
  )

  const handleLiveSpeech = useCallback(
    (speech: {
      id: string
      event_key: string
      body: string
      performance?: unknown
      merope_state?: unknown
    }) => {
      void Promise.all([
        import('../features/merope/faceSpeechArbitration'),
        import('../features/merope/agentFaceChannel'),
      ]).then(([{ deliverProactiveFace, faceSpeechGate }, { agentFace }]) => {
        deliverProactiveFace(agentFace, faceSpeechGate, {
          id: speech.id,
          eventKey: speech.event_key,
          body: speech.body,
          performance: speech.performance,
          meropeState: speech.merope_state,
        })
      })
    },
    [],
  )

  const notifCenter = useNotificationCenter({
    enabled: !!user,
    userId: user?.id,
    onNew: handleNewNotification,
    onLiveSpeech: handleLiveSpeech,
    onLiveSpeechMotion: (id, performance) => {
      void import('../features/merope/faceSpeechArbitration').then(
        ({ refineProactiveFace, faceSpeechGate }) => {
          refineProactiveFace(faceSpeechGate, id, performance)
        },
      )
    },
    onMeropeState: (state) => {
      void import('../features/merope/agentFaceChannel').then(({ agentFace }) => {
        agentFace.updateState(state)
      })
    },
    onMeropeResync: () => window.dispatchEvent(new Event(ADDRESSEE_UPDATED_EVENT)),
    includeInPanel: includeNotificationInPanel,
  })
  const { loaded: notifLoaded, loadHistory: loadNotifHistory } = notifCenter

  // hook 预载失败时，打开通知页再试一次。
  useEffect(() => {
    if (panelTab === 'notifications' && !notifLoaded) {
      void loadNotifHistory()
    }
  }, [panelTab, notifLoaded, loadNotifHistory])

  const validContents = useMemo(() => {
    return dynamicContents.filter((c) => {
      if (!c.icon || !c.text) return false
      if (typeof c.text === 'string' && c.text.trim().length === 0) return false
      if (!allowsIslandType(islandContent, String(c.type))) return false
      return true
    })
  }, [dynamicContents, islandContent])

  const textRef = useRef<HTMLSpanElement>(null)
  const [needsScroll, setNeedsScroll] = useState(false)

  const {
    canRefresh: canRefreshWallpaper,
    refreshWallpaper,
    loadWallpaper,
  } = useWallpaper()

  const musicPlayer = useMusicPlayer()
  // 解构稳定引用，避免 expand/collapse 依赖整个 musicPlayer。
  const { setProgressUiVisible } = musicPlayer

  const triggerRef = useRef<HTMLDivElement>(null)
  const expandedContentRef = useRef<HTMLDivElement>(null)
  // 镜像给 popstate 等原生回调，避免闭包过期。
  const isExpandedRef = useRef(false)
  // 展开时压哨兵历史，系统返回先收起面板。
  const historyArmedRef = useRef(false)
  // 自己 history.back() 的 popstate 计数：关→开时迟到回退不得再收起。
  const pendingBackRef = useRef(0)
  // morphing 必须是组件级 ref，局部变量会随测量 effect 重建丢失。
  const morphingRef = useRef(false)
  const perf = usePerformanceProfile()
  const anim = useAnimationLevel()

  // 同一套状态机按档位选 transition，不维护第二套交互。
  const motion = useMemo(
    () =>
      resolvePanelMotion({
        level: anim.level,
        reduceMotion: perf.reduceMotion,
      }),
    [anim.level, perf.reduceMotion],
  )
  // morph 开始时冻结档位跑完；中途降级会 cancel 尺寸过渡并误触发重测。
  const [activeMotion, setActiveMotion] = useState(motion)
  useEffect(() => {
    // 只在稳定态跟进档位，改档影响下一次交互。
    if (!isPanelMorphing(panel)) setActiveMotion(motion)
  }, [motion, panel.phase])

  const motionRef = useRef(activeMotion)
  useLayoutEffect(() => {
    motionRef.current = activeMotion
  }, [activeMotion])

  // 交接 delay/duration 按 morph 比例，两条时间线同步。
  const motionVars = useMemo(
    () =>
      ({
        '--gcp-morph': `${activeMotion.morphMs}ms`,
        '--gcp-tab': `${activeMotion.tabMs}ms`,
      }) as React.CSSProperties,
    [activeMotion],
  )

  const { preference: animPreference, togglePerformanceMode } =
    useAnimationPreference()
  const effectiveAnimationLevel =
    animPreference === 'auto' ? anim.level : animPreference
  const isStandardAnimation = effectiveAnimationLevel === 'standard'

  // layout effect 在同一次提交内、早于测量 effect 同步镜像。
  useLayoutEffect(() => {
    isExpandedRef.current = isPanelOpen(panel)
    historyArmedRef.current = panel.historyArmed
    morphingRef.current = isPanelMorphing(panel)
  }, [panel])

  useEffect(() => {
    loadDynamicContents()
    loadWallpaper()
  }, []) // 只在挂载跑一次，避免循环依赖。

  const dynamicContentProvider = getDynamicContentProvider()

  useEffect(() => {
    dynamicContentProvider.setLocale(locale)
  }, [locale, dynamicContentProvider])

  const loadIslandContent = useCallback(async () => {
    try {
      const cfg = (await getUIConfigDeduped()) as Record<string, unknown>
      setIslandContent(islandContentFromPublicUi(cfg))
    } catch {
      setIslandContent(DEFAULT_ISLAND_CONTENT)
    }
  }, [])

  useEffect(() => {
    void loadIslandContent()
    const onChanged = () => {
      void loadIslandContent()
    }
    window.addEventListener(ISLAND_CONTENT_CHANGED_EVENT, onChanged)
    return () => {
      window.removeEventListener(ISLAND_CONTENT_CHANGED_EVENT, onChanged)
    }
  }, [loadIslandContent])

  useEffect(() => {
    const unsubscribe = dynamicContentProvider.addListener((event) => {
      if (
        event.type === 'add' ||
        event.type === 'update' ||
        event.type === 'remove' ||
        event.type === 'clear'
      ) {
        refreshTappContents()
      }
    })

    return () => {
      unsubscribe()
    }
  }, [dynamicContentProvider])

  const refreshTappContents = useCallback(() => {
    safeSetDynamicContents((prev) => {
      const builtinContents = prev.filter(
        (c) => !c.type.toString().startsWith('tapp-'),
      )

      const tappContents = dynamicContentProvider
        .getAllContents()
        .filter((c) => c.type.toString().startsWith('tapp-'))
        .map((c) => ({
          type: c.type,
          icon: c.icon,
          text: c.text,
          subtext: c.subtext,
          showSubtext: c.showSubtext,
          sourceTappId: c.sourceTappId,
        }))

      return [...builtinContents, ...tappContents]
    })
  }, [dynamicContentProvider, safeSetDynamicContents])

  const getWeatherText = useCallback(
    (code: number): string => {
      const weatherT = t.weather ?? {}
      if (code === 0 || code === 1) return weatherT.sunny ?? 'Sunny'
      if (code === 2 || code === 3) return weatherT.cloudy ?? 'Cloudy'
      if (code === 45 || code === 48) return weatherT.foggy ?? 'Foggy'
      if (code >= 51 && code <= 67) return weatherT.rainy ?? 'Rainy'
      if (code >= 80 && code <= 82) return weatherT.rainy ?? 'Rainy'
      if (code >= 71 && code <= 77) return weatherT.snowy ?? 'Snowy'
      if (code >= 85 && code <= 86) return weatherT.snowy ?? 'Snowy'
      if (code >= 95 && code <= 99)
        return weatherT.thunderstorm ?? 'Thunderstorm'
      return weatherT.unavailable ?? 'Unknown'
    },
    [t.weather],
  )

  const renderWeatherIcon = useCallback((icon: string) => {
    return (
      <WeatherAssetIcon
        icon={icon}
        className="h-6 w-6 object-contain"
        fallbackClassName="dynamic-icon-emoji"
      />
    )
  }, [])

  const renderDynamicAssetIcon = useCallback((icon: string) => {
    return (
      <WeatherAssetIcon
        icon={icon}
        className="h-6 w-6 object-contain"
        fallbackClassName="dynamic-icon-emoji"
      />
    )
  }, [])

  const loadDynamicContents = useCallback(async () => {
    const contents: DynamicContent[] = []

    const greetingTranslations = {
      morning: t.greeting?.morning ?? 'Good morning',
      forenoon: t.greeting?.forenoon ?? t.greeting?.morning ?? 'Good morning',
      noon: t.greeting?.noon ?? 'Good afternoon',
      afternoon: t.greeting?.afternoon ?? 'Good afternoon',
      dusk: t.greeting?.dusk ?? t.greeting?.evening ?? 'Good evening',
      evening: t.greeting?.evening ?? 'Good evening',
      night: t.greeting?.night ?? 'Good night',
    }
    const greeting = getGreeting(user?.username, greetingTranslations, locale)
    let greetingIcon: string
    switch (greeting.icon) {
      case 'sunrise':
        greetingIcon = GREETING_ICON_ASSETS.sunrise
        break
      case 'sunset':
        greetingIcon = GREETING_ICON_ASSETS.sunset
        break
      case 'moon':
        greetingIcon = GREETING_ICON_ASSETS.moon
        break
      case 'cloud-sun':
        greetingIcon = GREETING_ICON_ASSETS.cloudSun
        break
      case 'sun':
      default:
        greetingIcon = GREETING_ICON_ASSETS.sun
        break
    }
    contents.push({
      type: 'greeting',
      icon: (
        <WeatherAssetIcon
          icon={greetingIcon}
          className="h-6 w-6 object-contain"
          fallbackClassName="dynamic-icon-emoji"
        />
      ),
      text: greeting.text || greetingTranslations.afternoon,
      subtext: greeting.time,
    })

    safeSetDynamicContents((prev) => {
      const preserved = prev.filter(
        (c) => c.type === 'music' || c.type.toString().startsWith('tapp-'),
      )
      return [...contents, ...preserved]
    })

    dynamicContentProvider.setContent('builtin', {
      type: 'greeting',
      icon: greeting.icon,
      text: greeting.text || greetingTranslations.afternoon,
      subtext: greeting.time,
      priority: 100,
    })

    if (weatherData) {
      const weatherText = `${weatherData.temperature} ${getWeatherText(weatherData.weatherCode)}`
      const weatherCity = weatherData.city || ''

      safeSetDynamicContents((prev) => {
        const filtered = prev.filter((c) => c.type !== 'weather')
        const greetingIndex = filtered.findIndex((c) => c.type === 'greeting')
        const insertIndex = greetingIndex >= 0 ? greetingIndex + 1 : 0
        return filtered.toSpliced(insertIndex, 0, {
          type: 'weather',
          icon: renderWeatherIcon(weatherData.icon),
          text: weatherText,
          subtext: weatherCity,
          showSubtext: true,
        })
      })

      dynamicContentProvider.setContent('builtin', {
        type: 'weather',
        icon: weatherData.icon,
        text: weatherText,
        subtext: weatherCity,
        priority: 90,
        showSubtext: true,
      })
    } else {
      loadResource.high('weather-info', async () => {
        try {
          const weather = await getWeatherInfo()
          if (weather) {
            setWeatherData(weather)

            const weatherText = `${weather.temperature} ${getWeatherText(weather.weatherCode)}`
            const weatherCity = weather.city || ''

            safeSetDynamicContents((prev) => {
              const filtered = prev.filter((c) => c.type !== 'weather')
              const greetingIndex = filtered.findIndex(
                (c) => c.type === 'greeting',
              )
              const insertIndex = greetingIndex >= 0 ? greetingIndex + 1 : 0
              return filtered.toSpliced(insertIndex, 0, {
                type: 'weather',
                icon: renderWeatherIcon(weather.icon),
                text: weatherText,
                subtext: weatherCity,
                showSubtext: true,
              })
            })

            dynamicContentProvider.setContent('builtin', {
              type: 'weather',
              icon: weather.icon,
              text: weatherText,
              subtext: weatherCity,
              priority: 90,
              showSubtext: true,
            })
          }
        } catch (error) {
          console.debug('[GlobalControlPanel] Weather unavailable:', error)
        }
      })
    }

    if (quoteData) {
      safeSetDynamicContents((prev) => {
        const filtered = prev.filter((c) => c.type !== 'quote')
        return [
          ...filtered,
          {
            type: 'quote',
            icon: renderDynamicAssetIcon(DYNAMIC_ICON_ASSETS.quote),
            text: quoteData.text,
            subtext: quoteData.author || undefined,
            showSubtext: false,
          },
        ]
      })

      dynamicContentProvider.setContent('builtin', {
        type: 'quote',
        icon: 'quote',
        text: quoteData.text,
        subtext: quoteData.author || undefined,
        priority: 50,
        showSubtext: false,
      })
    } else {
      loadResource.high('quote-info', async () => {
        try {
          const quote = await getRandomQuote(locale)
          if (quote?.text) {
            setQuoteData(quote)
            safeSetDynamicContents((prev) => {
              const filtered = prev.filter((c) => c.type !== 'quote')
              return [
                ...filtered,
                {
                  type: 'quote',
                  icon: renderDynamicAssetIcon(DYNAMIC_ICON_ASSETS.quote),
                  text: quote.text,
                  subtext: quote.author || undefined,
                  showSubtext: false,
                },
              ]
            })

            dynamicContentProvider.setContent('builtin', {
              type: 'quote',
              icon: 'quote',
              text: quote.text,
              subtext: quote.author || undefined,
              priority: 50,
              showSubtext: false,
            })
          }
        } catch (error) {
          console.debug('[GlobalControlPanel] Quote unavailable:', error)
        }
      })
    }

    refreshTappContents()
  }, [
    user?.username,
    t,
    locale,
    dynamicContentProvider,
    refreshTappContents,
    safeSetDynamicContents,
    weatherData,
    quoteData,
    getWeatherText,
    renderWeatherIcon,
    renderDynamicAssetIcon,
  ])

  useEffect(() => {
    if (user) {
      loadDynamicContents()
    }
  }, [user, loadDynamicContents])

  useEffect(() => {
    loadDynamicContents()
  }, [locale, loadDynamicContents])

  useEffect(() => {
    musicPlayer.loadMusicConfig()
  }, [])

  useEffect(() => {
    if (
      validContents.length > 0 &&
      currentContentIndex >= validContents.length
    ) {
      setCurrentContentIndex(0)
    }
  }, [validContents.length, currentContentIndex])

  useEffect(() => {
    if (validContents.length === 0 || isExpanded || isHovering) {
      // 淡出窗口内依赖变化会取消换页定时器，必须同步撤销淡出，否则内容停在 hidden。
      setIsTransitioning(false)
      return
    }

    let cycleTimerId: number | null = null
    let swapTimerId: number | null = null
    let revealTimerId: number | null = null
    let cancelled = false

    const clearTimers = () => {
      if (cycleTimerId !== null) {
        clearTimeout(cycleTimerId)
        cycleTimerId = null
      }
      if (swapTimerId !== null) {
        clearTimeout(swapTimerId)
        swapTimerId = null
      }
      if (revealTimerId !== null) {
        clearTimeout(revealTimerId)
        revealTimerId = null
      }
    }

    const cycle = () => {
      if (cancelled || document.hidden) return
      setIsTransitioning(true)
      swapTimerId = window.setTimeout(() => {
        swapTimerId = null
        if (cancelled) return
        setCurrentContentIndex((prev) => (prev + 1) % validContents.length)
        revealTimerId = window.setTimeout(() => {
          revealTimerId = null
          if (!cancelled) setIsTransitioning(false)
        }, 80)
        const base = 15000
        const nextDelay = Math.round(base * (anim.durationScale || 1))
        cycleTimerId = window.setTimeout(cycle, nextDelay)
      }, 300)
    }

    const startDelay = Math.round(6000 * (anim.durationScale || 1))
    cycleTimerId = window.setTimeout(cycle, startDelay)

    const handleVisibility = () => {
      if (document.hidden) {
        // 隐藏时中止过渡，避免停在已淡出未换页的中间态。
        clearTimers()
        setIsTransitioning(false)
      } else if (!cancelled) {
        clearTimers()
        setIsTransitioning(false)
        const restartDelay = Math.round(2000 * (anim.durationScale || 1))
        cycleTimerId = window.setTimeout(cycle, restartDelay)
      }
    }
    document.addEventListener('visibilitychange', handleVisibility)

    return () => {
      cancelled = true
      clearTimers()
      setIsTransitioning(false)
      document.removeEventListener('visibilitychange', handleVisibility)
    }
  }, [validContents.length, isExpanded, isHovering, anim.durationScale])

  useLayoutEffect(() => {
    if (!triggerRef.current) return
    const triggerEl = triggerRef.current

    if (!isExpanded) {
      triggerEl.style.height = '3rem'
      return
    }
    if (!expandedContentRef.current) return
    const contentEl = expandedContentRef.current

    let lastHeight = 0
    let lastUpdateTime = 0
    let pendingMeasure = false
    let measureTimeout: number | null = null
    let visibilityTimeout: number | null = null

    // 触控/低性能：加大节流，跳过 ResizeObserver。
    const isMobileDevice = perf.isMobile || isReducedAnimation(anim)

    const THROTTLE_MS = isMobileDevice ? 1200 : 600

    // duringMorph 与 immediate 独立：morph 中不写高度；immediate 只跳过节流，仍服从 morph 闸门。
    const measure = (opts?: { immediate?: boolean, duringMorph?: boolean }) => {
      if (morphingRef.current && !opts?.duringMorph) return

      const now = Date.now()
      if (now - lastUpdateTime < THROTTLE_MS && !opts?.immediate) {
        if (!pendingMeasure) {
          pendingMeasure = true
          const delay = THROTTLE_MS - (now - lastUpdateTime)
          measureTimeout = window.setTimeout(() => {
            pendingMeasure = false
            measureTimeout = null
            measure()
          }, delay)
        }
        return
      }
      lastUpdateTime = now

      // DEV：scrollHeight 依赖内容宽度钉在展开终值；改回 100%/flex 压缩会按中间帧算高。
      if (import.meta.env.DEV) {
        const rootFontSize = Number.parseFloat(
          getComputedStyle(document.documentElement).fontSize,
        )
        // 宽度与 CSS 一致：桌面 356px，移动 calc(100vw - 4.25rem)。
        const expectedWidth =
          window.innerWidth <= 640
            ? window.innerWidth - 4.25 * rootFontSize
            : 356
        if (Math.abs(contentEl.offsetWidth - expectedWidth) > 2) {
          console.warn(
            `[GlobalControlPanel] 面板内容宽度 ${contentEl.offsetWidth}px 偏离预期终值 ${Math.round(expectedWidth)}px：` +
              'scrollHeight 高度测量依赖 .expanded-panel-content 的固定宽度契约' +
              '（GlobalControlPanel.css），请勿改回 width: 100% 或移除 flex-shrink: 0',
          )
        }
      }

      // 内容宽度已钉在展开终值，直接读布局高度，勿再克隆到 body。
      const raw = contentEl.scrollHeight

      const compensated = Math.ceil(raw * CONTROL_PANEL_HEIGHT_COMPENSATION)

      if (Math.abs(compensated - lastHeight) > 4) {
        lastHeight = compensated
        triggerEl.style.height = `${compensated}px`
      }
    }

    measure({ immediate: true, duringMorph: true })

    // 动画结束后重测，补齐 morph 期间丢弃的内容变化。闸门读组件级 morphingRef。
    const handleAnimationEnd = () => {
      lastUpdateTime = 0
      measure({ immediate: true, duringMorph: true })
    }
    window.addEventListener('gcp-animation-end', handleAnimationEnd)

    const handleRemeasure = (e: Event) => {
      const detail = (e as CustomEvent<{ immediate?: boolean } | undefined>)
        .detail
      measure({ immediate: detail?.immediate })
    }
    window.addEventListener('gcp-remeasure', handleRemeasure)
    window.addEventListener('control-panel-content-resize', handleRemeasure)

    const handleViewportChange = () => {
      lastUpdateTime = 0
      measure()
    }
    window.addEventListener('resize', handleViewportChange)
    window.addEventListener('orientationchange', handleViewportChange)

    // 可见性重测定时器必须可清理，否则迟到的 measure 会朝旧外壳写高度。
    const handleVisibility = () => {
      if (!document.hidden) {
        lastUpdateTime = 0
        if (visibilityTimeout !== null) clearTimeout(visibilityTimeout)
        visibilityTimeout = window.setTimeout(() => {
          visibilityTimeout = null
          measure()
        }, 100)
      }
    }
    document.addEventListener('visibilitychange', handleVisibility)

    let unobserveResize: (() => void) | null = null
    if (!isMobileDevice) {
      unobserveResize = observeResize(contentEl, () => measure())
    }

    const mutationObserver = new MutationObserver(() => measure())
    mutationObserver.observe(contentEl, {
      childList: true,
    })

    return () => {
      window.removeEventListener('gcp-animation-end', handleAnimationEnd)
      window.removeEventListener('gcp-remeasure', handleRemeasure)
      window.removeEventListener(
        'control-panel-content-resize',
        handleRemeasure,
      )
      window.removeEventListener('resize', handleViewportChange)
      window.removeEventListener('orientationchange', handleViewportChange)
      document.removeEventListener('visibilitychange', handleVisibility)
      if (unobserveResize) {
        unobserveResize()
      }
      mutationObserver.disconnect()
      if (measureTimeout !== null) {
        clearTimeout(measureTimeout)
      }
      if (visibilityTimeout !== null) {
        clearTimeout(visibilityTimeout)
      }
    }
    // 壁纸项与管理员项在孙节点，MutationObserver 看不到；加入 deps 翻转后强制重测。
  }, [
    isExpanded,
    perf.highHardware,
    perf.isMobile,
    anim.level,
    canRefreshWallpaper,
    user?.is_admin,
  ])

  const [themePreference, setThemePreference] = useState<ThemePreference>(
    getStoredThemePreference,
  )

  const cycleTheme = useCallback(() => {
    const next =
      THEME_CYCLE[
        (THEME_CYCLE.indexOf(themePreference) + 1) % THEME_CYCLE.length
      ]
    setThemePreference(next)
    localStorage.setItem('theme', next)

    const dark =
      next === 'auto'
        ? window.matchMedia('(prefers-color-scheme: dark)').matches
        : next === 'dark'
    applyThemeClass(dark)
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.THEME_SWITCH, {
          target: next,
          throttleMs: 2000,
        })
      },
    )
  }, [themePreference])

  // 相位由外壳 transitionend 推进，定时器只兜底。
  useEffect(() => {
    if (!isPanelMorphing(panel)) return
    const el = triggerRef.current
    const generation = panel.generation
    const activeMotion = motionRef.current

    // 子组件仍按 gcp-animation-start/end 冻结引擎。
    window.dispatchEvent(new CustomEvent('gcp-animation-start'))

    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      window.dispatchEvent(new CustomEvent('gcp-animation-end'))
      dispatchPanel({ type: 'settle', generation })
    }

    // 结束信号用 width（双向都变）；打断时走 transitioncancel。
    const handleTransitionEnd = (e: TransitionEvent) => {
      if (e.target === el && e.propertyName === 'width') finish()
    }
    if (activeMotion.spatial && el) {
      el.addEventListener('transitionend', handleTransitionEnd)
    }
    // 兜底定时器：非空间档、后台节流、!important 盖住过渡。
    const fallback = window.setTimeout(finish, settleTimeoutMs(activeMotion))

    return () => {
      window.clearTimeout(fallback)
      el?.removeEventListener('transitionend', handleTransitionEnd)
      // 被抢占时补发 end，避免子组件停在冻结态。
      if (!settled) {
        window.dispatchEvent(new CustomEvent('gcp-animation-end'))
      }
    }
  }, [panel.phase, panel.generation])

  // html.gcp-panel-open：移动端全屏 TApp 会抢 hit-test，用它关 TApp pointer-events。
  useEffect(() => {
    const root = document.documentElement
    if (isExpanded) {
      root.classList.add('gcp-panel-open')
    } else {
      root.classList.remove('gcp-panel-open')
    }
    return () => {
      root.classList.remove('gcp-panel-open')
    }
  }, [isExpanded])

  // 通知 Tab 控制区只 opacity 隐藏：进度/引擎需对齐可见性，展开首帧即置位。
  const progressUiVisible = showsProgressUi(panel)
  useEffect(() => {
    setProgressUiVisible(progressUiVisible)
  }, [progressUiVisible, setProgressUiVisible])

  const collapsePanel = useCallback(() => {
    dispatchPanel({ type: 'close' })
  }, [])

  const expandPanel = useCallback((tab?: PanelTab) => {
    // 压哨兵历史，系统返回先收起。保留 router state。同一 tick 立即置位，避免连开压出多余哨兵。
    let historyArmed = false
    if (!isExpandedRef.current) {
      isExpandedRef.current = true
      try {
        const st = window.history.state
        window.history.pushState(
          { ...(st ?? {}), idx: (st?.idx ?? 0) + 1, __gcpPanel: true },
          '',
        )
        historyArmed = true
      } catch {
        historyArmed = false
      }
      historyArmedRef.current = historyArmed
    }
    dispatchPanel({ type: 'open', tab, historyArmed })
  }, [])

  const handleClosePanel = useCallback(() => {
    if (!isExpandedRef.current) return
    if (historyArmedRef.current) {
      // 先解除武装再消费哨兵；pendingBack 必须计数，置 1 会把后续 popstate 当成用户返回。
      historyArmedRef.current = false
      pendingBackRef.current += 1
      window.history.back()
    }
    // 立即清镜像：同一 tick 关→开时第二次调用仍读到 true 会再关一次。
    isExpandedRef.current = false
    dispatchPanel({ type: 'close' })
    setForegroundSurface('none')
  }, [])

  const handleTogglePanel = useCallback(
    (tab?: PanelTab) => {
      if (isExpandedRef.current) {
        handleClosePanel()
      } else {
        expandPanel(tab)
        setForegroundSurface(
          tab === 'notifications' ? 'notification' : 'control_panel',
        )
        void import('../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.CONTROL_PANEL_OPEN, {
              throttleMs: 5000,
            })
          },
        )
      }
    },
    [handleClosePanel, expandPanel],
  )

  useEffect(() => {
    const handlePopState = () => {
      // 自己 history.back() 的 popstate 只记账，不改状态。
      if (pendingBackRef.current > 0) {
        pendingBackRef.current -= 1
        return
      }
      if (!historyArmedRef.current) return
      historyArmedRef.current = false
      dispatchPanel({ type: 'close' })
    }
    window.addEventListener('popstate', handlePopState)
    return () => window.removeEventListener('popstate', handlePopState)
  }, [])

  // 面板内导航：收起并用目标路由替换哨兵，不能 history.back()（异步回退会吞掉随后的 push）。
  const handleNavigateFromPanel = useCallback(
    (path: string) => {
      const wasArmed = historyArmedRef.current
      historyArmedRef.current = false
      collapsePanel()
      navigate(path, { replace: wasArmed })
    },
    [collapsePanel, navigate],
  )

  const handleOpenConfig = useCallback(() => {
    handleNavigateFromPanel('/config')
  }, [handleNavigateFromPanel])

  const handleOpenNotifSession = useCallback(
    (sessionId: string, opts?: { runId?: string; taskId?: string }) => {
      handleClosePanel()
      window.dispatchEvent(
        new CustomEvent('arael-open-session', {
          detail: {
            sessionId,
            runId: opts?.runId,
            taskId: opts?.taskId,
          },
        }),
      )
    },
    [handleClosePanel],
  )

  const handleOpenAgentManage = useCallback(
    (tab?: 'heartbeat' | 'skills' | 'memory') => {
      handleClosePanel()
      window.dispatchEvent(
        new CustomEvent('arael-open-manage', {
          detail: tab ? { tab } : {},
        }),
      )
    },
    [handleClosePanel],
  )

  useEffect(() => {
    const handleOpenPanel = (e: Event) => {
      const tab = (e as CustomEvent<{ tab?: PanelTab } | undefined>).detail?.tab
      if (isExpandedRef.current) {
        if (tab) dispatchPanel({ type: 'selectTab', tab })
        return
      }
      handleTogglePanel(tab)
    }

    window.addEventListener('open-control-panel', handleOpenPanel)
    return () => {
      window.removeEventListener('open-control-panel', handleOpenPanel)
    }
  }, [handleTogglePanel])

  const tourPanelPose = useSyncExternalStore(
    subscribeTour,
    readHomeBrowseTourPanelPose,
    readHomeBrowseTourPanelPose,
  )
  const tourDrovePanel = useRef(false)

  useLayoutEffect(() => {
    if (tourPanelPose === 'expanded') {
      tourDrovePanel.current = true
      if (!isExpandedRef.current) {
        expandPanel('control')
        setForegroundSurface('control_panel')
        return
      }
      if (panelTab !== 'control') {
        dispatchPanel({ type: 'selectTab', tab: 'control' })
      }
      return
    }
    if (tourPanelPose === 'collapsed') {
      if (isExpandedRef.current) handleClosePanel()
      tourDrovePanel.current = false
      return
    }
    if (tourDrovePanel.current) {
      if (isExpandedRef.current) handleClosePanel()
      tourDrovePanel.current = false
    }
  }, [expandPanel, handleClosePanel, panelTab, tourPanelPose])

  // 收起时内联错误不可见，用 toast 兜底；展开时不重复弹。
  const musicErrorKey = musicPlayer.musicErrorKey
  const musicErrorDetail = musicPlayer.musicErrorDetail
  useEffect(() => {
    if (!musicErrorKey || isExpandedRef.current) return
    const musicT = t.music as Record<string, string> | undefined
    showToast({
      message: formatMusicError(
        musicT?.[musicErrorKey] ?? musicErrorKey,
        musicErrorDetail,
      ),
      type: 'error',
      duration: 5000,
    })
    // t 不入依赖：只在错误出现时弹一次，语言切换不重弹。
  }, [musicErrorDetail, musicErrorKey])

  const lastLyricTextRef = useRef<string>('')
  const lastSongIdRef = useRef<string>('')
  const lastPlayingStateRef = useRef<boolean>(false)

  useEffect(() => {
    const { currentSong, isPlaying, lyrics, currentLyricIndex } = musicPlayer

    if (isExpanded) return

    if (
      currentSong &&
      isPlaying &&
      lyrics.length > 0 &&
      currentLyricIndex >= 0
    ) {
      const currentLyric = lyrics[currentLyricIndex]

      if (lastLyricTextRef.current === currentLyric.text) {
        return
      }
      lastLyricTextRef.current = currentLyric.text
      lastSongIdRef.current = currentSong.id
      lastPlayingStateRef.current = true

      let lyricDuration = 5
      if (currentLyricIndex < lyrics.length - 1) {
        const nextLyric = lyrics[currentLyricIndex + 1]
        lyricDuration = Math.max(1, nextLyric.time - currentLyric.time)
      } else {
        lyricDuration = 8
      }

      // 播放中写歌词用函数式更新，避免闭包过期。
      safeSetDynamicContents((prev) => {
        const filtered = prev.filter((c) => c.type !== 'music')
        return [
          {
            type: 'music' as const,
            icon: renderDynamicAssetIcon(DYNAMIC_ICON_ASSETS.music),
            text: currentLyric.text,
            subtext: `${currentSong.name} - ${currentSong.artist}`,
            lyricDuration,
          },
          ...filtered,
        ]
      })
    } else if (currentSong) {
      const songChanged = lastSongIdRef.current !== currentSong.id
      const playingChanged = lastPlayingStateRef.current !== isPlaying

      if (!songChanged && !playingChanged && lastLyricTextRef.current === '') {
        return
      }

      lastLyricTextRef.current = ''
      lastSongIdRef.current = currentSong.id
      lastPlayingStateRef.current = isPlaying

      safeSetDynamicContents((prev) => {
        const filtered = prev.filter((c) => c.type !== 'music')
        return [
          {
            type: 'music' as const,
            icon: renderDynamicAssetIcon(
              isPlaying
                ? DYNAMIC_ICON_ASSETS.music
                : DYNAMIC_ICON_ASSETS.musicPaused,
            ),
            text: currentSong.name,
            subtext: currentSong.artist,
          },
          ...filtered,
        ]
      })
    } else if (lastSongIdRef.current !== '') {
      lastLyricTextRef.current = ''
      lastSongIdRef.current = ''
      lastPlayingStateRef.current = false
      safeSetDynamicContents((prev) => prev.filter((c) => c.type !== 'music'))
    }
  }, [
    musicPlayer.currentSong?.id,
    musicPlayer.lyrics.length,
    musicPlayer.currentLyricIndex,
    musicPlayer.isPlaying,
    isExpanded,
    safeSetDynamicContents,
    renderDynamicAssetIcon,
  ])

  const safeContentIndex =
    validContents.length > 0
      ? Math.min(currentContentIndex, validContents.length - 1)
      : 0

  const currentContent =
    validContents.length > 0 ? validContents[safeContentIndex] : null

  const prevLyricTextRef = useRef<string>('')
  const scrollResetKeyRef = useRef<number>(0)

  useEffect(() => {
    if (!textRef.current || !currentContent) return

    const element = textRef.current
    const isMusic = currentContent.type === 'music'
    const textChanged =
      isMusic &&
      prevLyricTextRef.current !== '' &&
      prevLyricTextRef.current !== currentContent.text

    if (textChanged) {
      element.classList.add('lyric-transition')
      const timer = setTimeout(() => {
        element.classList.remove('lyric-transition')
      }, 100)

      scrollResetKeyRef.current++

      return () => clearTimeout(timer)
    }

    if (isMusic) {
      prevLyricTextRef.current = currentContent.text
    } else {
      prevLyricTextRef.current = ''
    }
  }, [currentContent?.text, currentContent?.type])

  useEffect(() => {
    if (!textRef.current || !currentContent || isExpanded) {
      setNeedsScroll(false)
      return
    }

    const element = textRef.current

    const updateScrollAnimation = () => {
      let scrollHeight = 0
      let overflowAmount = 0
      let shouldScroll = false

      batchRead(() => {
        const twoLineHeight = 34
        scrollHeight = element.scrollHeight
        overflowAmount = scrollHeight - twoLineHeight
        shouldScroll = overflowAmount > 5
      })

      batchWrite(() => {
        if (shouldScroll) {
          element.style.setProperty('--scroll-distance', `-${overflowAmount}px`)

          let duration: number
          let delay: string

          if (currentContent.type === 'music' && currentContent.lyricDuration) {
            duration = Math.max(1.5, currentContent.lyricDuration - 0.5)
            delay = '0.3s'
          } else if (currentContent.type === 'music') {
            duration = 4
            delay = '0.5s'
          } else {
            duration = Math.max(
              10,
              Math.min(20, Math.ceil(overflowAmount / 20) + 10),
            )
            delay = '1.5s'
          }

          element.style.setProperty('--scroll-duration', `${duration}s`)
          element.style.setProperty('--scroll-delay', delay)

          setNeedsScroll(false)
          requestAnimationFrame(() => {
            setNeedsScroll(true)
          })
        } else {
          element.style.removeProperty('--scroll-distance')
          element.style.removeProperty('--scroll-duration')
          element.style.removeProperty('--scroll-delay')
          setNeedsScroll(false)
        }
      })
    }

    const unobserve = observeResize(
      element,
      (_entry) => {
        updateScrollAnimation()
      },
      { immediate: true },
    )

    updateScrollAnimation()

    return () => {
      unobserve()
    }
  }, [
    isExpanded,
    currentContent?.text,
    currentContent?.type,
    currentContent?.lyricDuration,
    currentContentIndex,
    scrollResetKeyRef.current,
  ])

  const shouldShowSubtext = useCallback((content: DynamicContent): boolean => {
    if (content.showSubtext !== undefined) {
      return content.showSubtext
    }

    if (content.type.toString().startsWith('tapp-')) {
      return !!content.subtext
    }

    const typesWithSubtext: DynamicContentType[] = ['weather', 'theme']
    return typesWithSubtext.includes(content.type)
  }, [])

  const hasValidContent = validContents.length > 0

  const notifCount = notifCenter.items.length
  const notificationIndicator =
    notifCount > 0 ? (
      <span className="dynamic-arrow-badge">
        {notifCount > 99 ? '99+' : notifCount}
      </span>
    ) : (
      <svg
        className="dynamic-arrow"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M19 9l-7 7-7-7"
        />
      </svg>
    )
  const residentLabel = format(t.controlPanel.residentRunning, {
    count: backgroundResidents.length,
  })
  const collapsedIndicator = (
    <span className="dynamic-status-indicators">
      {backgroundResidents.length > 0 && (
        <button
          type="button"
          className="tapp-resident-indicator"
          aria-label={residentLabel}
          title={residentLabel}
          onClick={(event) => {
            event.stopPropagation()
            handleTogglePanel(
              islandPanelTabForClick({ affordance: 'notification' }),
            )
          }}
        >
          {backgroundResidents.length}
        </button>
      )}
      <button
        type="button"
        className="dynamic-notification-affordance"
        aria-label={t.notificationCenter.title}
        onClick={(event) => {
          event.stopPropagation()
          handleTogglePanel(
            islandPanelTabForClick({ affordance: 'notification' }),
          )
        }}
      >
        {notificationIndicator}
      </button>
    </span>
  )

  return (
    <React.Fragment>
      <div className="global-control-bar">
        <div className="control-bar-content">
          <div
            ref={triggerRef}
            data-tour="control-island"
            className={[
              'control-bar-trigger',
              isExpanded ? 'expanded' : '',
              // morph 中冻结 hover/active 变换。
              isPanelMorphing(panel) ? 'gcp-animating' : '',
              panel.phase === 'closing' ? 'gcp-closing' : '',
              activeMotion.spatial ? '' : 'gcp-no-morph',
            ]
              .filter(Boolean)
              .join(' ')}
            style={motionVars}
            onPointerEnter={(e) => {
              if (isHoverCapablePointer(e.pointerType)) setIsHovering(true)
            }}
            onPointerLeave={() => setIsHovering(false)}
          >
            {hasValidContent && currentContent && (
              <div
                className={`dynamic-content-wrapper ${!showDynamicContent || isTransitioning ? 'hidden' : ''}`}
                onClick={() => {
                  handleTogglePanel(
                    islandPanelTabForClick({
                      affordance: 'control',
                      carouselType: currentContent.type,
                      viewport: navLayout,
                    }),
                  )
                }}
              >
                <span className="dynamic-icon">{currentContent.icon}</span>
                <div className="dynamic-text">
                  <span
                    ref={textRef}
                    className={`dynamic-text-main ${needsScroll ? 'scrolling' : ''}`}
                  >
                    {currentContent.text}
                  </span>
                  {currentContent.subtext &&
                    shouldShowSubtext(currentContent) && (
                      <span className="dynamic-text-sub">
                        {currentContent.subtext}
                      </span>
                    )}
                </div>
                {collapsedIndicator}
              </div>
            )}

            {/* 无内容也保留点击区；不按 isExpanded 卸载，否则展开首帧硬切。隐藏走绝对定位 + opacity:0。 */}
            {!hasValidContent && (
              <div
                className={`dynamic-content-wrapper empty-state ${!showDynamicContent ? 'hidden' : ''}`}
                onClick={() =>
                  handleTogglePanel(
                    islandPanelTabForClick({
                      affordance: 'control',
                      viewport: navLayout,
                    }),
                  )
                }
              >
                {collapsedIndicator}
              </div>
            )}

            <div
              ref={expandedContentRef}
              className={`expanded-panel-content ${showPanelContent ? 'visible' : ''}`}
              data-tour="control-panel"
            >
              <div className="control-panel-header">
                <UserSection
                  onClosePanel={handleClosePanel}
                  onNavigateFromPanel={handleNavigateFromPanel}
                />
                <button
                  type="button"
                  onClick={(e) => {
                    // 避免 touch 残留 focus / 父级 :active 干扰收起 morph。
                    e.stopPropagation()
                    ;(e.currentTarget as HTMLButtonElement).blur()
                    handleClosePanel()
                  }}
                  className="control-close-btn"
                  aria-label={t.common.close}
                >
                  <svg
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                    aria-hidden
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2.25}
                      d="M5 15l7-7 7 7"
                    />
                  </svg>
                </button>
              </div>

              <div
                className="notif-tab-bar"
                role="tablist"
                data-tab={panelTab}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={panelTab === 'control'}
                  className={`notif-tab ${panelTab === 'control' ? 'active' : ''}`}
                  onClick={() => {
                    dispatchPanel({ type: 'selectTab', tab: 'control' })
                    setForegroundSurface('control_panel')
                  }}
                >
                  {t.notificationCenter.tabControl}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={panelTab === 'notifications'}
                  className={`notif-tab ${panelTab === 'notifications' ? 'active' : ''}`}
                  onClick={() => {
                    dispatchPanel({ type: 'selectTab', tab: 'notifications' })
                    setForegroundSurface('notification')
                    void import('../utils/analyticsEvents').then(
                      ({ trackProductEvent, AnalyticsEvents }) => {
                        trackProductEvent(AnalyticsEvents.NOTIFICATION_OPEN, {
                          throttleMs: 5000,
                        })
                      },
                    )
                  }}
                >
                  {t.notificationCenter.title}
                  {notifCount > 0 && (
                    <span className="notif-tab-badge">
                      {notifCount > 99 ? '99+' : notifCount}
                    </span>
                  )}
                  {backgroundResidents.length > 0 && (
                    <span
                      className="tapp-resident-tab-badge"
                      aria-label={residentLabel}
                      title={residentLabel}
                    >
                      {backgroundResidents.length}
                    </span>
                  )}
                </button>
              </div>

              {/* 控制区始终挂载定高；切走用容器 opacity:0，勿 display:none（小组件会测成 0）或卸载。 */}
              <div className="notif-panel-body">
                <div
                  className={`notif-control-content ${
                    panelTab === 'notifications' ? 'inactive' : ''
                  }`}
                  inert={panelTab === 'notifications'}
                >
                  {showControlPanelWidgets && (
                    <ControlPanelWidgets
                      isAdmin={user?.is_admin}
                      panelVisible={progressUiVisible}
                    />
                  )}

                  <MusicPlayer
                    player={musicPlayer}
                    panelVisible={progressUiVisible}
                  />

                  <ControlQuickActions
                    locale={locale}
                    labels={t.controlPanel}
                    onLocaleChange={setLocale}
                    isDark={isDark}
                    themePreference={themePreference}
                    onCycleTheme={cycleTheme}
                    animPreference={animPreference}
                    animLevel={anim.level}
                    isStandardAnimation={isStandardAnimation}
                    onToggleAnimation={togglePerformanceMode}
                    canRefreshWallpaper={canRefreshWallpaper}
                    onRefreshWallpaper={refreshWallpaper}
                    isAdmin={!!user?.is_admin}
                    onOpenConfig={handleOpenConfig}
                  />
                </div>

                {/* 通知层 inset:0 跟高；首次进入后保持挂载，tab 往返交叉淡入；收起才卸载。 */}
                {mountsNotifications(panel) && (
                  <div
                    className={`notif-overlay ${panelTab === 'notifications' ? 'active' : ''}`}
                    inert={panelTab !== 'notifications'}
                  >
                    <NotificationPanelList
                      center={notifCenter}
                      fill
                      onOpenSession={handleOpenNotifSession}
                      onNavigate={handleNavigateFromPanel}
                      onOpenAgentManage={handleOpenAgentManage}
                      browserNotificationsEnabled={
                        notificationPreferences.delivery.browser
                      }
                    />
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div
        className={[
          'control-panel-overlay',
          showOverlay ? 'visible' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        onClick={handleClosePanel}
      />
    </React.Fragment>
  )
}

export default GlobalControlPanel
