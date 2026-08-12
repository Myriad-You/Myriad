import type { DynamicContentType } from '../services/DynamicContentProvider'

import type { AppNotification } from '../services/notificationApi'
import type { QuoteData, WeatherData } from '../utils/dynamicContent'
import type { PanelTab } from './ControlPanel/panelTransition'
import React, {
  lazy,
  Suspense,
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
  shouldDeliverNotification,
} from '../services/notificationDelivery'
import {
  getGreeting,
  getRandomQuote,
  getWeatherInfo,
  WEATHER_ICON_ASSETS,
} from '../utils/dynamicContent'
import {
  getNavLayoutSnapshot,
  getServerNavLayoutSnapshot,
  subscribeNavLayout,
} from '../utils/navLayout'
import { loadResource } from '../utils/resourceLoader'
import { useThemeMode } from '../utils/themeSubscriber'
import { showToast } from '../utils/toastManager'
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
  showsOverlayBlur,
  showsPanelContent,
  showsProgressUi,
} from './ControlPanel/panelTransition'
import { UserSection } from './ControlPanel/UserSection'
import NotificationPanelList from './NotificationPanelList'
import {
  NotificationSourceIcon,
  notificationSourceIconAsset,
} from './notifications/NotificationIcons'
import { WeatherAssetIcon } from './weather/WeatherAssetIcon'
import './GlobalControlPanel.css'

// 懒加载展开面板子组件 — 仅在用户展开面板时加载
const ControlPanelWidgets = lazy(() =>
  import('./ControlPanel/ControlPanelWidgets').then((m) => ({
    default: m.ControlPanelWidgets,
  })),
)
const MusicPlayer = lazy(() =>
  import('./ControlPanel/MusicPlayer').then((m) => ({
    default: m.MusicPlayer,
  })),
)

const GREETING_ICON_ASSETS = {
  sunrise: '/icons/greeting/sunrise.webp',
  sun: WEATHER_ICON_ASSETS.sunny,
  cloudSun: WEATHER_ICON_ASSETS.partlyCloudy,
  sunset: '/icons/greeting/sunset.webp',
  moon: '/icons/greeting/night.webp',
} as const

const CONTROL_PANEL_ICON_ASSETS = {
  appearanceLight: WEATHER_ICON_ASSETS.sunny,
  appearanceDark: GREETING_ICON_ASSETS.moon,
  animationStandard: '/icons/control-panel/animation-standard.webp',
  animationLight: '/icons/control-panel/animation-light.webp',
  language: '/icons/control-panel/language.webp',
  wallpaper: '/icons/control-panel/wallpaper.webp',
  config: '/icons/control-panel/config.webp',
} as const

const DYNAMIC_ICON_ASSETS = {
  quote: '/icons/dynamic/quote.webp',
  music: '/icons/dynamic/music.webp',
  musicPaused: '/icons/dynamic/music-paused.webp',
} as const

/** 外观偏好：浅色 / 深色 / 跟随系统 */
type ThemePreference = 'light' | 'dark' | 'auto'

const THEME_CYCLE: ThemePreference[] = ['light', 'dark', 'auto']

function getStoredThemePreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'auto'
  const stored = localStorage.getItem('theme')
  return stored === 'light' || stored === 'dark' ? stored : 'auto'
}

/** 应用主题 class 并同步 meta theme-color */
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

/** 扩展的动态内容类型（包含 Tapp 自定义类型） */
interface DynamicContent {
  type: DynamicContentType
  icon: React.ReactNode
  text: string
  subtext?: string
  /** 是否显示副文本 */
  showSubtext?: boolean
  /** 来源 Tapp ID */
  sourceTappId?: string
  /** 歌词持续时间（秒）- 仅用于 music 类型 */
  lyricDuration?: number
}

const GlobalControlPanel: React.FC = () => {
  const navigate = useNavigate()
  const { user } = useAuth()
  const { locale, setLocale, t } = useI18n()
  const navLayout = useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  // Mobile / touch-tablet: skip control-panel widget grid (weather/quote etc.)
  // to save vertical space and memory; music player + settings remain.
  const showControlPanelWidgets = navLayout === 'desktop'
  const { preferences: notificationPreferences } = useNotificationPreferences(
    user?.id,
  )
  // 展开/收起的唯一状态所有者（issue #320）。
  // 遮罩、收缩内容、展开内容、进度 UI、动画类名全部从 phase 派生，
  // 不再由若干独立 boolean + 固定 setTimeout 各自维护。
  const [panel, dispatchPanel] = useReducer(panelReducer, initialPanelState)
  const isExpanded = isPanelOpen(panel)
  const showDynamicContent = showsDynamicContent(panel)
  const showPanelContent = showsPanelContent(panel)
  const showOverlay = showsOverlay(panel)
  /** 面板 tab：控制面板 / 通知 */
  const panelTab = panel.tab
  // 使用共享主题订阅器，避免创建多余的 MutationObserver
  const isDark = useThemeMode()

  // 页面可见性状态 - 用于冻结动态内容更新
  const [isPageVisible, setIsPageVisible] = useState(!document.hidden)
  const pendingUpdatesRef = useRef<
    Array<(prev: DynamicContent[]) => DynamicContent[]>
  >([])

  // 监听页面可见性变化
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

  // 动态内容状态
  const [dynamicContents, setDynamicContents] = useState<DynamicContent[]>([])
  const [currentContentIndex, setCurrentContentIndex] = useState(0)
  const [isHovering, setIsHovering] = useState(false)
  const [isTransitioning, setIsTransitioning] = useState(false)
  const [weatherData, setWeatherData] = useState<WeatherData | null>(null)
  const [quoteData, setQuoteData] = useState<QuoteData | null>(null)

  // 安全的动态内容更新函数 - 页面隐藏时暂存更新
  const safeSetDynamicContents = useCallback(
    (updater: (prev: DynamicContent[]) => DynamicContent[]) => {
      if (document.hidden) {
        // 页面隐藏时，暂存更新
        pendingUpdatesRef.current.push(updater)
      } else {
        // 页面可见时，直接应用更新
        setDynamicContents(updater)
      }
    },
    [],
  )

  // 页面恢复可见时，应用所有暂存的更新
  useEffect(() => {
    if (isPageVisible && pendingUpdatesRef.current.length > 0) {
      // 先截取并清空队列，再把稳定快照交给 React。
      // setState 的函数式 updater 不保证在调用点同步执行；若在其外部先清空
      // pendingUpdatesRef，稍后执行的 updater 会读到空数组，导致后台期间积累的
      // 问候语、天气、通知等更新全部丢失。
      const pendingUpdates = pendingUpdatesRef.current
      pendingUpdatesRef.current = []

      // 合并所有暂存的更新
      setDynamicContents((prev) => {
        let result = prev
        for (const updater of pendingUpdates) {
          result = updater(result)
        }
        return result
      })
    }
  }, [isPageVisible])

  // 通知中心

  const notifCarouselTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )

  // 卸载时清理轮播撤下定时器
  useEffect(
    () => () => {
      if (notifCarouselTimerRef.current) {
        clearTimeout(notifCarouselTimerRef.current)
      }
    },
    [],
  )

  /** 新通知到达：按统一投递策略分发到面板之外的展示位置。 */
  const handleNewNotification = useCallback(
    (n: AppNotification) => {
      const source = notificationSourceFor(n)
      const icon = (
        <NotificationSourceIcon source={source} className="h-4 w-4" />
      )
      const snippet = n.body.length > 60 ? `${n.body.slice(0, 60)}…` : n.body

      // 1. 接入智能岛轮播（置顶展示，20 秒后自动撤下）
      if (shouldDeliverNotification(notificationPreferences, n, 'island')) {
        safeSetDynamicContents((prev) => [
          {
            type: 'notification',
            icon,
            text: n.title,
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

      // 2. 所有允许投递到 Toast 的通知都走同一全局容器。
      // 通知优先级只决定视觉类型，不再决定通知是否展示。
      if (shouldDeliverNotification(notificationPreferences, n, 'toast')) {
        const showInPanel = shouldDeliverNotification(
          notificationPreferences,
          n,
          'panel',
        )
        showToast({
          title: n.title,
          message: snippet,
          type: notificationToastType(n),
          duration: 6000,
          showCloseButton: true,
          onClick: showInPanel
            ? () => {
                // 复用既有的打开面板事件并带上目标 tab：
                // 收起时展开直达通知页，已展开时只做一次 tab 交叉淡入
                window.dispatchEvent(
                  new CustomEvent('open-control-panel', {
                    detail: { tab: 'notifications' },
                  }),
                )
              }
            : undefined,
        })
      }

      // 3. 页面在后台时推浏览器系统通知
      if (
        document.hidden &&
        shouldDeliverNotification(notificationPreferences, n, 'browser') &&
        typeof Notification !== 'undefined' &&
        Notification.permission === 'granted'
      ) {
        try {
          // 构造即展示（无需持有实例），tag 去重同 id 通知
          void new Notification(n.title, {
            body: n.body.slice(0, 200),
            tag: n.id,
            icon: notificationSourceIconAsset(source),
          })
        } catch {
          /* 某些环境不支持构造 Notification，忽略 */
        }
      }
    },
    [notificationPreferences, safeSetDynamicContents],
  )

  const includeNotificationInPanel = useCallback(
    (notification: AppNotification) =>
      shouldDeliverNotification(notificationPreferences, notification, 'panel'),
    [notificationPreferences],
  )

  const notifCenter = useNotificationCenter({
    enabled: !!user,
    userId: user?.id,
    onNew: handleNewNotification,
    includeInPanel: includeNotificationInPanel,
  })
  const { loaded: notifLoaded, loadHistory: loadNotifHistory } = notifCenter

  // 历史已在 hook 启用时预载；此处仅在预载失败（loaded 仍为 false）时
  // 于打开通知页时重试
  useEffect(() => {
    if (panelTab === 'notifications' && !notifLoaded) {
      void loadNotifHistory()
    }
  }, [panelTab, notifLoaded, loadNotifHistory])

  // 高度策略：控制面板内容始终挂载并定义面板高度（切走时仅 visibility:hidden，
  // 布局保留、懒加载的配置项出现时照常触发重测）；通知作为绝对定位覆盖层
  // 盖在其上（inset:0 自动跟随面板高度）。tab 切换本身不触发任何重算。

  // 过滤掉空白内容，获取有效的动态内容列表（提前定义，供轮播逻辑使用）
  const validContents = useMemo(() => {
    return dynamicContents.filter((c) => {
      // 必须有图标和文本
      if (!c.icon || !c.text) return false
      // 文本不能是空字符串或只有空白
      if (typeof c.text === 'string' && c.text.trim().length === 0) return false
      return true
    })
  }, [dynamicContents])

  // 文本引用，用于检测是否需要滚动
  const textRef = useRef<HTMLSpanElement>(null)
  const [needsScroll, setNeedsScroll] = useState(false)

  // 壁纸管理 Hook（替代之前的独立状态和函数）
  const {
    canRefresh: canRefreshWallpaper,
    refreshWallpaper,
    loadWallpaper,
  } = useWallpaper()

  // 音乐播放器 Hook（从 GlobalControlPanel 分离）
  const musicPlayer = useMusicPlayer()
  // 解构出稳定引用，供 expand/collapse 回调使用而不引入 musicPlayer 对象依赖
  const { setProgressUiVisible } = musicPlayer

  // DOM 引用
  const triggerRef = useRef<HTMLDivElement>(null)
  const expandedContentRef = useRef<HTMLDivElement>(null)
  // 展开状态镜像（供 popstate 等原生事件回调读取，避免闭包过期）
  const isExpandedRef = useRef(false)
  // 是否已压入哨兵历史记录（面板展开时移动端系统返回应先收起面板）
  const historyArmedRef = useRef(false)
  // 自己调用 history.back() 产生的 popstate 计数：快速「关→开」时，
  // 迟到的那次回退不得把刚展开的面板再收掉
  const pendingBackRef = useRef(0)
  // 外壳 morph 进行中（供测量闭包读取）。必须是组件级 ref：
  // 局部变量会随测量 effect 重建而丢失已经发生的开始信号
  const morphingRef = useRef(false)
  const perf = usePerformanceProfile()
  const anim = useAnimationLevel()

  // 动效档位：同一套状态机按 motion / performance policy 选择 transition profile，
  // 不维护第二套交互实现
  const motion = useMemo(
    () =>
      resolvePanelMotion({
        level: anim.level,
        reduceMotion: perf.reduceMotion,
        isMobile: perf.isMobile,
      }),
    [anim.level, perf.reduceMotion, perf.isMobile],
  )
  // 时长以 CSS 变量下发：内容交接的 delay/duration 全部按 morph 比例计算，
  // 保证两条时间线永远同步（不会因为改时长而错位）
  const motionVars = useMemo(
    () =>
      ({
        '--gcp-morph': `${motion.morphMs}ms`,
        '--gcp-tab': `${motion.tabMs}ms`,
      }) as React.CSSProperties,
    [motion],
  )

  const { preference: animPreference, togglePerformanceMode } =
    useAnimationPreference()
  const effectiveAnimationLevel =
    animPreference === 'auto' ? anim.level : animPreference
  const isStandardAnimation = effectiveAnimationLevel === 'standard'
  const animationModeClass = isStandardAnimation
    ? 'performance-standard'
    : 'performance-light'

  // 原生事件回调（popstate）与测量闭包读取的状态镜像。
  // 用 layout effect 保证在同一次提交内、且早于下方的测量 effect 生效
  useLayoutEffect(() => {
    isExpandedRef.current = isPanelOpen(panel)
    historyArmedRef.current = panel.historyArmed
    morphingRef.current = isPanelMorphing(panel)
  }, [panel])

  useEffect(() => {
    // 主题状态现在由 useThemeMode() hook 自动管理
    // 认证检查现在由 AuthContext 管理，用户信息会自动同步

    // 加载动态内容
    loadDynamicContents()

    // 加载壁纸配置以初始化 canRefresh 状态
    loadWallpaper()
  }, []) // 只在挂载时运行一次，避免循环依赖

  // 注意：壁纸颜色提取完全由 AppLayout 负责
  // GlobalControlPanel 不再处理壁纸颜色，只处理音乐封面颜色
  // 音量弹层开关 / 点外关闭在 MusicPlayer 内（state 与 ref 均来自 useMusicPlayer）

  // 获取动态内容提供者
  const dynamicContentProvider = getDynamicContentProvider()

  // 同步语言设置到动态内容提供者
  useEffect(() => {
    dynamicContentProvider.setLocale(locale)
  }, [locale, dynamicContentProvider])

  // 订阅 Tapp 动态内容更新
  useEffect(() => {
    const unsubscribe = dynamicContentProvider.addListener((event) => {
      // 当 Tapp 内容更新时，刷新动态内容列表
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

  // 刷新 Tapp 提供的动态内容
  const refreshTappContents = useCallback(() => {
    safeSetDynamicContents((prev) => {
      // 移除旧的 Tapp 内容
      const builtinContents = prev.filter(
        (c) => !c.type.toString().startsWith('tapp-'),
      )

      // 获取所有 Tapp 内容
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

      // 合并内容
      return [...builtinContents, ...tappContents]
    })
  }, [dynamicContentProvider, safeSetDynamicContents])

  // 天气文本翻译辅助函数（提取出来以便复用）
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
        className="h-6 w-6 object-contain drop-shadow-sm"
        fallbackClassName="dynamic-icon-emoji"
      />
    )
  }, [])

  const renderDynamicAssetIcon = useCallback((icon: string) => {
    return (
      <WeatherAssetIcon
        icon={icon}
        className="h-6 w-6 object-contain drop-shadow-sm"
        fallbackClassName="dynamic-icon-emoji"
      />
    )
  }, [])

  // 加载动态内容
  const loadDynamicContents = useCallback(async () => {
    const contents: DynamicContent[] = []

    // 1. 问候语（始终显示，立即加载）
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
          className="h-6 w-6 object-contain drop-shadow-sm"
          fallbackClassName="dynamic-icon-emoji"
        />
      ),
      text: greeting.text || greetingTranslations.afternoon,
      subtext: greeting.time,
    })

    // 立即显示问候语（保留音乐和 Tapp 内容，只更新内置内容）
    safeSetDynamicContents((prev) => {
      // 保留音乐和 Tapp 类型的内容
      const preserved = prev.filter(
        (c) => c.type === 'music' || c.type.toString().startsWith('tapp-'),
      )
      return [...contents, ...preserved]
    })

    // 同步问候语到动态内容提供者（供 Tapp 读取）
    dynamicContentProvider.setContent('builtin', {
      type: 'greeting',
      icon: greeting.icon,
      text: greeting.text || greetingTranslations.afternoon,
      subtext: greeting.time,
      priority: 100,
    })

    // 2. 天气信息（高优先级）
    // 如果已有天气数据，直接使用缓存数据更新（语言切换时）
    if (weatherData) {
      const weatherText = `${weatherData.temperature} ${getWeatherText(weatherData.weatherCode)}`
      const weatherCity = weatherData.city || ''

      safeSetDynamicContents((prev) => {
        // 移除旧的天气内容，添加新的翻译版本
        const filtered = prev.filter((c) => c.type !== 'weather')
        // 在问候语后插入天气信息
        const greetingIndex = filtered.findIndex((c) => c.type === 'greeting')
        const insertIndex = greetingIndex >= 0 ? greetingIndex + 1 : 0
        filtered.splice(insertIndex, 0, {
          type: 'weather',
          icon: renderWeatherIcon(weatherData.icon),
          text: weatherText,
          subtext: weatherCity,
          showSubtext: true,
        })
        return filtered
      })

      // 同步到动态内容提供者
      dynamicContentProvider.setContent('builtin', {
        type: 'weather',
        icon: weatherData.icon,
        text: weatherText,
        subtext: weatherCity,
        priority: 90,
        showSubtext: true,
      })
    } else {
      // 首次加载天气数据
      loadResource.high('weather-info', async () => {
        try {
          const weather = await getWeatherInfo()
          if (weather) {
            setWeatherData(weather)

            const weatherText = `${weather.temperature} ${getWeatherText(weather.weatherCode)}`
            const weatherCity = weather.city || ''

            safeSetDynamicContents((prev) => {
              // 移除旧的天气内容（如果有）
              const filtered = prev.filter((c) => c.type !== 'weather')
              // 在问候语后插入天气信息
              const greetingIndex = filtered.findIndex(
                (c) => c.type === 'greeting',
              )
              const insertIndex = greetingIndex >= 0 ? greetingIndex + 1 : 0
              filtered.splice(insertIndex, 0, {
                type: 'weather',
                icon: renderWeatherIcon(weather.icon),
                text: weatherText,
                subtext: weatherCity,
                showSubtext: true,
              })
              return filtered
            })

            // 同步到动态内容提供者
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
          // 静默处理错误 - 天气不可用时不显示
          console.debug('[GlobalControlPanel] Weather unavailable:', error)
        }
      })
    }

    // 3. 一言警句（高优先级）
    // 如果已有一言数据，直接复用（一言不需要翻译，只有备用句子需要根据语言切换）
    if (quoteData) {
      safeSetDynamicContents((prev) => {
        // 移除旧的一言内容，重新添加
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

      // 同步到动态内容提供者
      dynamicContentProvider.setContent('builtin', {
        type: 'quote',
        icon: 'quote',
        text: quoteData.text,
        subtext: quoteData.author || undefined,
        priority: 50,
        showSubtext: false,
      })
    } else {
      // 首次加载一言数据
      loadResource.high('quote-info', async () => {
        try {
          const quote = await getRandomQuote(locale)
          if (quote && quote.text) {
            setQuoteData(quote)
            safeSetDynamicContents((prev) => {
              // 移除旧的一言内容（如果有）
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

            // 同步到动态内容提供者
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
          // 静默处理错误 - 一言不可用时不显示
          console.debug('[GlobalControlPanel] Quote unavailable:', error)
        }
      })
    }

    // 4. 加载 Tapp 提供的动态内容
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

  // 当用户信息更新时，重新加载动态内容
  useEffect(() => {
    if (user) {
      loadDynamicContents()
    }
  }, [user, loadDynamicContents])

  // 当语言变化时，重新加载动态内容以更新问候语、天气等文本
  useEffect(() => {
    loadDynamicContents()
    // loadDynamicContents 依赖 t 和 locale，当语言变化时会自动使用新的翻译
  }, [locale, loadDynamicContents])

  // 壁纸加载和刷新功能已由 useWallpaper Hook 提供
  // loadWallpaperConfig 和 refreshWallpaper 已废弃

  // 初始化时加载音乐配置（只在挂载时运行一次）
  useEffect(() => {
    musicPlayer.loadMusicConfig()
  }, []) // 仅在组件挂载时运行一次

  // 确保 currentContentIndex 在有效范围内（使用 validContents）
  useEffect(() => {
    if (
      validContents.length > 0 &&
      currentContentIndex >= validContents.length
    ) {
      setCurrentContentIndex(0)
    }
  }, [validContents.length, currentContentIndex])

  // 动态内容轮播（带淡入淡出效果）- 仅在有有效内容时运行
  useEffect(() => {
    // 在以下情况禁用轮播：展开面板 / 悬停 / 有效内容为空 / 页面隐藏
    if (validContents.length === 0 || isExpanded || isHovering) {
      // 若依赖恰好在 300ms 淡出窗口内变化，上一轮 effect 会取消换页定时器；
      // 此处必须同步撤销淡出，否则内容会一直停留在 hidden 状态。
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
        // 稍等一帧后开始淡入，确保内容已更新
        revealTimerId = window.setTimeout(() => {
          revealTimerId = null
          if (!cancelled) setIsTransitioning(false)
        }, 80)
        // 下一次循环：延长停留时间到 15秒，低端设备 30秒
        const base = 15000
        const nextDelay = Math.round(base * (anim.durationScale || 1))
        cycleTimerId = window.setTimeout(cycle, nextDelay)
      }, 300)
    }

    // 首次延迟启动，等待 6 秒后开始轮播
    const startDelay = Math.round(6000 * (anim.durationScale || 1))
    cycleTimerId = window.setTimeout(cycle, startDelay)

    const handleVisibility = () => {
      if (document.hidden) {
        // 页面隐藏时中止完整过渡，避免停在已淡出但尚未换页的中间态
        clearTimers()
        setIsTransitioning(false)
      } else if (!cancelled) {
        // 页面重新可见时重新启动轮播
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

  // 主题变化已通过 useThemeMode() hook 自动响应
  // 无需额外的 MutationObserver

  // 动态计算展开面板的高度 -  事件驱动，无轮询
  // 外部可通过 dispatchEvent(new CustomEvent('gcp-remeasure')) 触发重测
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

    // 移动端 / 低性能模式：加大节流、跳过 ResizeObserver
    const isMobileDevice = perf.isMobile || isReducedAnimation(anim)

    // 节流时间：防止短时间内多次事件触发重复测量
    // 加大节流时间，减少克隆测量频率
    const THROTTLE_MS = isMobileDevice ? 1200 : 600

    const measure = (force = false) => {
      // morph 期间不写高度：目标值留给动画结束后的 measure(true) 一次性应用，
      // 异步到达的用户/配置/音乐/通知内容不会在过渡中途顶跳外壳
      if (morphingRef.current && !force) return

      const now = Date.now()
      if (now - lastUpdateTime < THROTTLE_MS && !force) {
        // 如果在节流期内,标记待测量,稍后执行
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

      // DEV 契约断言：scrollHeight 直读的正确性依赖 CSS 把内容宽度
      // 固定为展开终值（.expanded-panel-content 的 width + flex-shrink: 0）。
      // 若被改回 100% 或恢复 flex 压缩，高度会按动画中间帧计算——
      // 在开发环境立即暴露，生产零开销
      if (import.meta.env.DEV) {
        const rootFontSize = Number.parseFloat(
          getComputedStyle(document.documentElement).fontSize,
        )
        // 桌面 356px；移动端 calc(100vw - 4.25rem)，与 CSS 保持一致
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

      // 内容宽度已由 CSS 固定为展开终值（不随容器动画变化），
      // 直接读取真实布局高度即可 —— 无需克隆整棵面板到 body 测量，
      // 每次测量从"深克隆 + 插入 + 强制布局 + 移除"降为一次布局读取
      const raw = contentEl.scrollHeight

      // 适当补偿 (考虑内边距 + 过渡)
      const compensated = Math.ceil(raw * 1.08)

      if (Math.abs(compensated - lastHeight) > 4) {
        lastHeight = compensated
        triggerEl.style.height = `${compensated}px`
      }
    }

    // 立即测量，确保动画起始帧即为正确高度
    measure(true)

    // 动画结束后精确重测一次，补齐 morph 期间被丢弃的内容变化。
    // 注意：闸门读的是组件级 morphingRef，本 effect 因依赖变化重建时
    // 不会丢失「当前正在动画」这一事实（旧实现的局部标记会被重置为 false）
    const handleAnimationEnd = () => {
      lastUpdateTime = 0
      measure(true)
    }
    window.addEventListener('gcp-animation-end', handleAnimationEnd)

    // 统一使用事件驱动重测（移除轮询）
    const handleRemeasure = () => {
      measure()
    }
    window.addEventListener('gcp-remeasure', handleRemeasure)
    // 兼容 ControlPanelWidgets 触发的事件
    window.addEventListener('control-panel-content-resize', handleRemeasure)

    // 视口变化事件
    const handleViewportChange = () => {
      lastUpdateTime = 0 // 重置节流
      measure()
    }
    window.addEventListener('resize', handleViewportChange)
    window.addEventListener('orientationchange', handleViewportChange)

    // 可见性变化时重新测量
    const handleVisibility = () => {
      if (!document.hidden) {
        lastUpdateTime = 0
        setTimeout(measure, 100)
      }
    }
    document.addEventListener('visibilitychange', handleVisibility)

    // 桌面端：使用 ResizeObserver 监听内容尺寸变化
    let unobserveResize: (() => void) | null = null
    if (!isMobileDevice) {
      unobserveResize = observeResize(contentEl, () => measure())
    }

    // MutationObserver：只监听直接子节点变化
    const mutationObserver = new MutationObserver(() => measure())
    mutationObserver.observe(contentEl, {
      childList: true,
      // 不监听 subtree 和 characterData，减少触发频率
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
    }
    // canRefreshWallpaper / user?.is_admin：壁纸配置与用户信息均为异步加载，
    // 壁纸切换/系统配置两个控制项会在面板展开后才出现。它们渲染在
    // .control-items-grid 内部（contentEl 的孙节点），MutationObserver
    // （仅监听直接子节点）观察不到，移动端又没有 ResizeObserver 兜底——
    // 历史上全靠 ×1.08 的冗余高度硬扛，不够时按钮被裁掉"第一时间不显示"。
    // 加入 deps 后翻转即触发 measure(true) 精确重测
  }, [
    isExpanded,
    perf.highHardware,
    perf.isMobile,
    anim.level,
    canRefreshWallpaper,
    user?.is_admin,
  ])

  // 外观偏好（浅色/深色/自动），isDark 始终反映当前实际外观
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
    // isDark 状态由 useThemeMode() hook 自动响应 class 变化，无需手动 setIsDark
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

  // 相位推进：由外壳真实的过渡结束事件驱动，定时器只作兜底。
  // 收缩内容淡出 → 外壳 morph → 展开内容淡入 → 遮罩，共用同一条时间线，
  // 不再有「先出空壳、400ms 后内容突然加入」的固定猜测。
  useEffect(() => {
    if (!isPanelMorphing(panel)) return
    const el = triggerRef.current
    const generation = panel.generation

    // 子组件（MusicPlayer 频谱/歌词引擎）仍按这两个事件冻结
    window.dispatchEvent(new CustomEvent('gcp-animation-start'))

    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      window.dispatchEvent(new CustomEvent('gcp-animation-end'))
      dispatchPanel({ type: 'settle', generation })
    }

    // width 在两个方向上都必然变化（160px ↔ 400px），是最可靠的结束信号；
    // 过渡被打断时浏览器发 transitioncancel，由新一轮 effect 接管
    const handleTransitionEnd = (e: TransitionEvent) => {
      if (e.target === el && e.propertyName === 'width') finish()
    }
    if (motion.spatial && el) {
      el.addEventListener('transitionend', handleTransitionEnd)
    }
    // 兜底：非空间档位没有尺寸过渡、后台标签页被节流、过渡被 !important 覆盖
    const fallback = window.setTimeout(finish, settleTimeoutMs(motion))

    return () => {
      window.clearTimeout(fallback)
      el?.removeEventListener('transitionend', handleTransitionEnd)
      // 被新动作抢占：补发 end，避免子组件停在冻结态
      if (!settled) {
        window.dispatchEvent(new CustomEvent('gcp-animation-end'))
      }
    }
  }, [panel.phase, panel.generation, motion])

  // 展开态写入 html.gcp-panel-open：全屏 TApp iframe 在移动端会抢 hit-test，
  // 宿主侧用该 class 临时关闭 TApp 层 pointer-events（见 GlobalControlPanel.css）
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

  // 通知 Tab 下控制区仅 opacity 隐藏：进度 tick / 引擎需与可见性对齐。
  // 展开首帧即置位，展开内容淡入时 currentTime 已是最新值
  const progressUiVisible = showsProgressUi(panel)
  useEffect(() => {
    setProgressUiVisible(progressUiVisible)
  }, [progressUiVisible, setProgressUiVisible])

  const collapsePanel = useCallback(() => {
    dispatchPanel({ type: 'close' })
  }, [])

  const expandPanel = useCallback((tab?: PanelTab) => {
    // 压入哨兵历史记录：移动端系统返回时先收起面板，而非直接离开页面
    // 保留 react-router 写入的 state（usr/key/idx），避免破坏其内部索引
    let historyArmed = false
    if (!isExpandedRef.current) {
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
    }
    dispatchPanel({ type: 'open', tab, historyArmed })
  }, [])

  const handleClosePanel = useCallback(() => {
    if (!isExpandedRef.current) return
    if (historyArmedRef.current) {
      // 先解除武装再消费哨兵记录；pendingBack 让「关→立刻开」时迟到的
      // popstate 不会把刚展开的面板再收掉
      historyArmedRef.current = false
      pendingBackRef.current = 1
      window.history.back()
    }
    dispatchPanel({ type: 'close' })
  }, [])

  const handleTogglePanel = useCallback(
    (tab?: PanelTab) => {
      if (isExpandedRef.current) {
        handleClosePanel()
      } else {
        expandPanel(tab)
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

  // 系统返回（popstate）时收起面板 — 复用同一条收起动画路径
  useEffect(() => {
    const handlePopState = () => {
      // 自己调用 history.back() 产生的那次回退：只记账，不改状态
      if (pendingBackRef.current > 0) {
        pendingBackRef.current = 0
        return
      }
      if (!historyArmedRef.current) return
      historyArmedRef.current = false
      dispatchPanel({ type: 'close' })
    }
    window.addEventListener('popstate', handlePopState)
    return () => window.removeEventListener('popstate', handlePopState)
  }, [])

  // 从面板内部导航到其他页面：收起面板，并用目标路由替换哨兵记录
  // （不能走 handleClosePanel 的 history.back()——异步回退会吞掉紧随其后的 push）
  const handleNavigateFromPanel = useCallback(
    (path: string) => {
      const wasArmed = historyArmedRef.current
      historyArmedRef.current = false
      collapsePanel()
      navigate(path, { replace: wasArmed })
    },
    [collapsePanel, navigate],
  )

  // 点击任务类通知：收起面板并打开对应 Arael 会话（AraelPanel 监听该事件；可带 run/task 以 reattach）
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

  const handleOpenAraelManage = useCallback(
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

  // 监听打开控制面板事件（来自音乐小组件等点击）
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

  // 音乐错误兜底提示：面板收起时 MusicPlayer 的内联错误不可见
  // （典型场景：Agent 触发歌单加载失败），用全局 toast 兜底；
  // 面板展开时已有内联提示，不重复弹
  const musicErrorKey = musicPlayer.musicErrorKey
  useEffect(() => {
    if (!musicErrorKey || isExpandedRef.current) return
    const musicT = t.music as Record<string, string> | undefined
    showToast({
      message: musicT?.[musicErrorKey] ?? musicErrorKey,
      type: 'error',
      duration: 5000,
    })
    // t 不入依赖：只在错误出现时弹一次，语言切换不重弹
  }, [musicErrorKey])

  // 当有歌词时，更新动态内容以显示歌词（仅播放时）
  // 使用 useRef 来减少状态更新频率
  const lastLyricTextRef = useRef<string>('')
  const lastSongIdRef = useRef<string>('')
  const lastPlayingStateRef = useRef<boolean>(false)

  useEffect(() => {
    const { currentSong, isPlaying, lyrics, currentLyricIndex } = musicPlayer

    // 早期返回：面板展开时不更新动态内容
    // 注意：页面隐藏时由 safeSetDynamicContents 自动暂存更新
    if (isExpanded) return

    if (
      currentSong &&
      isPlaying &&
      lyrics.length > 0 &&
      currentLyricIndex >= 0
    ) {
      const currentLyric = lyrics[currentLyricIndex]

      // 如果歌词文本没有变化，跳过更新（避免重复渲染）
      if (lastLyricTextRef.current === currentLyric.text) {
        return
      }
      lastLyricTextRef.current = currentLyric.text
      lastSongIdRef.current = currentSong.id
      lastPlayingStateRef.current = true

      // 计算当前歌词的持续时间（到下一句歌词的时间差）
      let lyricDuration = 5 // 默认5秒
      if (currentLyricIndex < lyrics.length - 1) {
        const nextLyric = lyrics[currentLyricIndex + 1]
        lyricDuration = Math.max(1, nextLyric.time - currentLyric.time)
      } else {
        // 最后一句歌词，默认8秒
        lyricDuration = 8
      }

      // 播放时显示歌词 - 使用函数式更新避免闭包问题
      safeSetDynamicContents((prev) => {
        const filtered = prev.filter((c) => c.type !== 'music')
        return [
          {
            type: 'music' as const,
            icon: renderDynamicAssetIcon(DYNAMIC_ICON_ASSETS.music),
            text: currentLyric.text,
            subtext: `${currentSong.name} - ${currentSong.artist}`,
            lyricDuration, // 传入歌词持续时间
          },
          ...filtered,
        ]
      })
    } else if (currentSong) {
      // 避免重复更新：检查歌曲和播放状态是否真的变化了
      const songChanged = lastSongIdRef.current !== currentSong.id
      const playingChanged = lastPlayingStateRef.current !== isPlaying

      if (!songChanged && !playingChanged && lastLyricTextRef.current === '') {
        return
      }

      // 重置歌词文本引用
      lastLyricTextRef.current = ''
      lastSongIdRef.current = currentSong.id
      lastPlayingStateRef.current = isPlaying

      // 暂停时或没有歌词时只显示歌曲名
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
      // 没有歌曲时移除音乐内容（仅当之前有歌曲时）
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

  // 确保索引在有效范围内
  const safeContentIndex =
    validContents.length > 0
      ? Math.min(currentContentIndex, validContents.length - 1)
      : 0

  // 获取当前显示的动态内容
  const currentContent =
    validContents.length > 0 ? validContents[safeContentIndex] : null

  // 用于跟踪上一次歌词文本，实现切换时的淡入淡出
  const prevLyricTextRef = useRef<string>('')
  // 用于强制触发滚动动画重置的key
  const scrollResetKeyRef = useRef<number>(0)

  // 歌词切换时的淡入淡出效果（独立处理，不影响滚动检测）
  useEffect(() => {
    if (!textRef.current || !currentContent) return

    const element = textRef.current
    const isMusic = currentContent.type === 'music'
    const textChanged =
      isMusic &&
      prevLyricTextRef.current !== '' &&
      prevLyricTextRef.current !== currentContent.text

    if (textChanged) {
      // 歌词切换时添加淡入淡出效果
      element.classList.add('lyric-transition')
      const timer = setTimeout(() => {
        element.classList.remove('lyric-transition')
      }, 100)

      // 强制触发滚动重置
      scrollResetKeyRef.current++

      return () => clearTimeout(timer)
    }

    // 更新上一次歌词文本
    if (isMusic) {
      prevLyricTextRef.current = currentContent.text
    } else {
      prevLyricTextRef.current = ''
    }
  }, [currentContent?.text, currentContent?.type])

  // 检测文本是否超出2行，需要垂直滚动 - 使用统一动画调度器优化性能
  useEffect(() => {
    if (!textRef.current || !currentContent) {
      setNeedsScroll(false)
      return
    }

    const element = textRef.current

    // 滚动检测和动画配置函数
    const updateScrollAnimation = () => {
      let scrollHeight = 0
      let overflowAmount = 0
      let shouldScroll = false

      // 批量读取阶段 - 使用统一调度器避免布局抖动
      batchRead(() => {
        const twoLineHeight = 34
        scrollHeight = element.scrollHeight
        overflowAmount = scrollHeight - twoLineHeight
        shouldScroll = overflowAmount > 5
      })

      // 批量写入阶段
      batchWrite(() => {
        if (shouldScroll) {
          // 设置 CSS 变量来控制垂直滚动距离
          element.style.setProperty('--scroll-distance', `-${overflowAmount}px`)

          // 根据内容类型计算滚动时间和延迟
          let duration: number
          let delay: string

          // 歌词特殊处理：使用精确的时间轴同步
          if (currentContent.type === 'music' && currentContent.lyricDuration) {
            // 歌词：使用歌词持续时间（到下一句的时间差）
            // 减去0.5秒作为缓冲，留出0.3秒作为延迟，确保流畅过渡
            duration = Math.max(1.5, currentContent.lyricDuration - 0.5)
            delay = '0.3s' // 歌词用更短的延迟，快速响应
          } else if (currentContent.type === 'music') {
            // 没有时间轴的歌词（最后一句或无时间戳），默认 4 秒
            duration = 4
            delay = '0.5s'
          } else {
            // 普通内容：根据溢出量动态计算，每20px需要1秒，最短10秒，最长20秒
            duration = Math.max(
              10,
              Math.min(20, Math.ceil(overflowAmount / 20) + 10),
            )
            delay = '1.5s'
          }

          element.style.setProperty('--scroll-duration', `${duration}s`)
          element.style.setProperty('--scroll-delay', delay)

          // 重置滚动动画（确保从头开始）
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

    // 使用统一调度器的ResizeObserver，共享Observer实例，性能更优
    const unobserve = observeResize(
      element,
      (_entry) => {
        updateScrollAnimation()
      },
      { immediate: true },
    ) // 立即执行首次测量

    // 监听内容变化，强制重新计算滚动
    // 这确保歌词切换时滚动动画会重置
    updateScrollAnimation()

    return () => {
      unobserve()
    }
  }, [
    currentContent?.text,
    currentContent?.type,
    currentContent?.lyricDuration,
    currentContentIndex,
    scrollResetKeyRef.current,
  ])

  /**
   * 判断是否应该显示副文本
   * - 显式指定 showSubtext 时使用指定值
   * - 默认规则：天气、主题、Tapp 内容显示副文本；问候语、一言、音乐不显示
   */
  const shouldShowSubtext = useCallback((content: DynamicContent): boolean => {
    // 显式指定时使用指定值
    if (content.showSubtext !== undefined) {
      return content.showSubtext
    }

    // Tapp 类型内容默认显示副文本
    if (content.type.toString().startsWith('tapp-')) {
      return !!content.subtext
    }

    // 默认规则：天气和主题显示副文本
    const typesWithSubtext: DynamicContentType[] = ['weather', 'theme']
    return typesWithSubtext.includes(content.type)
  }, [])

  // 是否有有效内容可显示
  const hasValidContent = validContents.length > 0

  // 收缩态右侧指示器：有通知时下箭头替换为计数徽标
  // （无已读概念：计数 = 未清除的通知数，iOS 通知中心模型）
  const notifCount = notifCenter.items.length
  const collapsedIndicator =
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

  return (
    <React.Fragment>
      {/* 顶部控制栏 - 智能岛 */}
      <div className="global-control-bar">
        <div className="control-bar-content">
          <div
            ref={triggerRef}
            className={[
              'control-bar-trigger',
              isExpanded ? 'expanded' : '',
              // morph 进行中：冻结 hover/active 变换，按档位决定是否停背景模糊
              isPanelMorphing(panel) ? 'gcp-animating' : '',
              panel.phase === 'closing' ? 'gcp-closing' : '',
              motion.spatial ? '' : 'gcp-no-morph',
              motion.blurDuringMorph ? '' : 'gcp-freeze-blur',
            ]
              .filter(Boolean)
              .join(' ')}
            style={motionVars}
            onMouseEnter={() => setIsHovering(true)}
            onMouseLeave={() => setIsHovering(false)}
          >
            {/* 动态轮播内容 - 仅在有有效内容时显示 */}
            {hasValidContent && currentContent && (
              <div
                className={`dynamic-content-wrapper ${!showDynamicContent || isTransitioning ? 'hidden' : ''}`}
                onClick={() => {
                  // 点击轮播中的通知内容 → 直接进入通知 tab（与展开同一次状态切换）
                  handleTogglePanel(
                    currentContent.type === 'notification'
                      ? 'notifications'
                      : undefined,
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
                  {/* 根据 showSubtext 属性或类型判断是否显示副文本 */}
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

            {/* 无有效内容时，仍需保持可点击区域以展开面板。
                不按 isExpanded 卸载：卸载会让收缩内容在展开首帧硬切消失，
                这里交给 showDynamicContent 走与外壳同一条时间线的淡出/淡入
                （隐藏态是绝对定位 + opacity:0 + pointer-events:none，无布局代价） */}
            {!hasValidContent && (
              <div
                className={`dynamic-content-wrapper empty-state ${!showDynamicContent ? 'hidden' : ''}`}
                onClick={() => handleTogglePanel()}
              >
                {collapsedIndicator}
              </div>
            )}

            {/* 展开的控制面板内容 - 通过 JS 控制显示/隐藏 */}
            <div
              ref={expandedContentRef}
              className={`expanded-panel-content ${showPanelContent ? 'visible' : ''}`}
            >
              {/* 头部 - 用户信息按钮 */}
              <div className="control-panel-header">
                <UserSection
                  onClosePanel={handleClosePanel}
                  onNavigateFromPanel={handleNavigateFromPanel}
                />
                <button
                  type="button"
                  onClick={(e) => {
                    // 避免 touch 残留 focus / 父级 :active 干扰收起 morph
                    e.stopPropagation()
                    ;(e.currentTarget as HTMLButtonElement).blur()
                    handleClosePanel()
                  }}
                  className="control-close-btn"
                  aria-label={t.common.close}
                >
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
                      d="M6 18L18 6M6 6l12 12"
                    />
                  </svg>
                </button>
              </div>

              {/* Tab 切换：控制面板 / 通知 */}
              <div className="notif-tab-bar" role="tablist">
                <button
                  type="button"
                  role="tab"
                  aria-selected={panelTab === 'control'}
                  className={`notif-tab ${panelTab === 'control' ? 'active' : ''}`}
                  onClick={() =>
                    dispatchPanel({ type: 'selectTab', tab: 'control' })}
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
                </button>
              </div>

              {/* 内容区：控制面板内容始终挂载并定义高度；
                  通知作为绝对定位覆盖层盖在其上，高度自动跟随。
                  控制内容切走时用容器级 opacity:0（保留布局，懒加载的
                  配置项出现时仍会触发重测），而非 display:none（会把
                  小组件测成 0 尺寸）或卸载（切回时引发连环重算）。
                  inert 负责把隐藏内容移出焦点链与无障碍树 */}
              <div className="notif-panel-body">
                <div
                  className={`notif-control-content ${
                    panelTab === 'notifications' ? 'inactive' : ''
                  }`}
                  inert={panelTab === 'notifications'}
                >
                  {/* 动态信息卡片 - 仅桌面显示；移动端关闭以省高度与资源 */}
                  <Suspense fallback={null}>
                    {showControlPanelWidgets && (
                      <ControlPanelWidgets isAdmin={user?.is_admin} />
                    )}

                    {/* 音乐播放器：收起或非控制 Tab 时停频谱/歌词引擎，不刷进度 */}
                    <MusicPlayer
                      player={musicPlayer}
                      panelVisible={progressUiVisible}
                    />
                  </Suspense>

                  {/* 控制项网格 - 一行两个 */}
                  <div className="control-items-grid">
                    {/* 主题切换 - 循环：浅色 → 深色 → 自动；图标始终反映当前实际外观 */}
                    <div className="control-item control-item-compact">
                      <div className="control-item-info">
                        <div className="control-item-icon icon-theme">
                          <WeatherAssetIcon
                            icon={
                              isDark
                                ? CONTROL_PANEL_ICON_ASSETS.appearanceDark
                                : CONTROL_PANEL_ICON_ASSETS.appearanceLight
                            }
                            className="h-full w-full object-contain"
                          />
                        </div>
                        <div>
                          <h4 className="control-item-title">
                            {t.controlPanel.appearance}
                          </h4>
                          <p className="control-item-desc">
                            {themePreference === 'auto'
                              ? t.controlPanel.auto
                              : themePreference === 'dark'
                                ? t.controlPanel.dark
                                : t.controlPanel.light}
                          </p>
                        </div>
                      </div>
                      <button
                        onClick={cycleTheme}
                        className="control-action-btn"
                        aria-label={t.controlPanel.themeSwitch}
                      >
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
                            d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                          />
                        </svg>
                      </button>
                    </div>

                    {/* 动效等级切换 */}
                    <div className="control-item control-item-compact">
                      <div className="control-item-info">
                        <div
                          className={`control-item-icon icon-performance ${animationModeClass}`}
                        >
                          <WeatherAssetIcon
                            icon={
                              isStandardAnimation
                                ? CONTROL_PANEL_ICON_ASSETS.animationStandard
                                : CONTROL_PANEL_ICON_ASSETS.animationLight
                            }
                            className="h-full w-full object-contain"
                          />
                        </div>
                        <div>
                          <h4 className="control-item-title">
                            {t.controlPanel.animation}
                          </h4>
                          <p className="control-item-desc">
                            {animPreference === 'auto'
                              ? anim.level === 'standard'
                                ? t.controlPanel.highPerformance
                                : t.controlPanel.lowPerformance
                              : animPreference === 'light'
                                ? t.controlPanel.lowPerformance
                                : t.controlPanel.highPerformance}
                          </p>
                        </div>
                      </div>
                      <button
                        onClick={togglePerformanceMode}
                        className={`control-toggle animation-toggle ${animationModeClass} ${isStandardAnimation ? 'active' : ''}`}
                        aria-label={t.controlPanel.animation}
                      >
                        <span className="control-toggle-slider"></span>
                      </button>
                    </div>

                    {/* 语言切换 */}
                    <div className="control-item control-item-compact">
                      <div className="control-item-info">
                        <div className="control-item-icon icon-language">
                          <WeatherAssetIcon
                            icon={CONTROL_PANEL_ICON_ASSETS.language}
                            className="h-full w-full object-contain"
                          />
                        </div>
                        <div>
                          <h4 className="control-item-title">
                            {t.controlPanel.language}
                          </h4>
                          <p className="control-item-desc">
                            {locale === 'zh-CN'
                              ? '简体中文'
                              : locale === 'ja-JP'
                                ? '日本語'
                                : 'English'}
                          </p>
                        </div>
                      </div>
                      <button
                        onClick={() => {
                          // 循环切换语言列表
                          const locales = ['zh-CN', 'en-US', 'ja-JP'] as const
                          const currentIndex = locales.indexOf(locale)
                          const nextIndex = (currentIndex + 1) % locales.length
                          setLocale(locales[nextIndex])
                        }}
                        onWheel={(e) => {
                          e.preventDefault()
                          const locales = ['zh-CN', 'en-US', 'ja-JP'] as const
                          const currentIndex = locales.indexOf(locale)
                          // 向下滚动 = 下一个，向上滚动 = 上一个
                          const nextIndex =
                            e.deltaY > 0
                              ? (currentIndex + 1) % locales.length
                              : (currentIndex - 1 + locales.length) %
                                locales.length
                          setLocale(locales[nextIndex])
                        }}
                        className="language-switch-btn"
                        aria-label={t.controlPanel.languageSwitch}
                      >
                        <span className="language-code">
                          {locale === 'zh-CN'
                            ? '中'
                            : locale === 'ja-JP'
                              ? '日'
                              : 'En'}
                        </span>
                      </button>
                    </div>

                    {/* 壁纸切换 - 仅在非单一图片链接时显示 */}
                    {/* Debug: canRefreshWallpaper = {String(canRefreshWallpaper)} */}
                    {canRefreshWallpaper && (
                      <div className="control-item control-item-compact">
                        <div className="control-item-info">
                          <div className="control-item-icon icon-wallpaper">
                            <WeatherAssetIcon
                              icon={CONTROL_PANEL_ICON_ASSETS.wallpaper}
                              className="h-full w-full object-contain"
                            />
                          </div>
                          <div>
                            <h4 className="control-item-title">
                              {t.controlPanel.wallpaper}
                            </h4>
                            <p className="control-item-desc">
                              {t.controlPanel.random}
                            </p>
                          </div>
                        </div>
                        <button
                          onClick={refreshWallpaper}
                          className="control-action-btn"
                          aria-label={t.controlPanel.wallpaperSwitch}
                        >
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
                              d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                            />
                          </svg>
                        </button>
                      </div>
                    )}

                    {/* 系统配置 - 仅管理员可见 */}
                    {user?.is_admin && (
                      <div className="control-item control-item-compact">
                        <div className="control-item-info">
                          <div className="control-item-icon icon-config">
                            <WeatherAssetIcon
                              icon={CONTROL_PANEL_ICON_ASSETS.config}
                              className="h-full w-full object-contain"
                            />
                          </div>
                          <div>
                            <h4 className="control-item-title">
                              {t.controlPanel.configuration}
                            </h4>
                            <p className="control-item-desc">
                              {t.controlPanel.system}
                            </p>
                          </div>
                        </div>
                        <button
                          onClick={() => handleNavigateFromPanel('/config')}
                          className="control-action-btn"
                          aria-label={t.controlPanel.configuration}
                        >
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
                              d="M13 7l5 5m0 0l-5 5m5-5H6"
                            />
                          </svg>
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                {/* 通知覆盖层：inset:0 跟随控制面板高度，列表内部滚动。
                    首次进入通知页后保持挂载：tab 往返是同一表面上的交叉淡入，
                    而不是「立即卸载 + 条件挂载」的硬切；收起到 collapsed 时随
                    状态机复位一起卸载，回收列表资源 */}
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
                      onOpenAraelManage={handleOpenAraelManage}
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

      {/* 遮罩层 - 始终存在，通过 CSS 控制显示。
          defer-blur 档位（移动端 / 低性能 / reduced-motion）把全屏 backdrop-filter
          移出 morph 热路径，稳定展开后再淡入模糊；桌面标准档观感不变 */}
      <div
        className={[
          'control-panel-overlay',
          showOverlay ? 'visible' : '',
          motion.blurDuringMorph ? '' : 'defer-blur',
          showsOverlayBlur(panel, motion) ? 'blurred' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        style={motionVars}
        onClick={handleClosePanel}
      />
    </React.Fragment>
  )
}

export default GlobalControlPanel
