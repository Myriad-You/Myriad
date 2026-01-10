import type { DynamicContentType } from '../services/DynamicContentProvider'
import type {
  QuoteData,
  WeatherData,
} from '../utils/dynamicContent'
import type { User } from './ControlPanel/UserSection'
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAnimationPreference } from '../contexts/AnimationPreferenceContext'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { batchRead, batchWrite, observeResize } from '../hooks/animation'
import { useAnimationLevel } from '../hooks/useAnimationLevel'
import { useMusicPlayer } from '../hooks/useMusicPlayer'
import { usePerformanceProfile } from '../hooks/usePerformanceProfile'
import { useWallpaper } from '../hooks/useWallpaper'
import {

  getDynamicContentProvider,
} from '../services/DynamicContentProvider'
import {
  getGreeting,
  getRandomQuote,
  getWeatherInfo,
} from '../utils/dynamicContent'
import { loadResource } from '../utils/resourceLoader'
import { useThemeMode } from '../utils/themeSubscriber'
import { ControlPanelWidgets } from './ControlPanel/ControlPanelWidgets'
import { MusicPlayer } from './ControlPanel/MusicPlayer'
import { UserSection } from './ControlPanel/UserSection'
import './GlobalControlPanel.css'

/** 扩展的动态内容类型（包含 Tapp 自定义类型） */
interface DynamicContent {
  type: DynamicContentType
  icon: string
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
  const { user: authUser } = useAuth()
  const { locale, setLocale, t } = useI18n()
  const [isExpanded, setIsExpanded] = useState(false)
  const [showDynamicContent, setShowDynamicContent] = useState(true)
  const [showPanelContent, setShowPanelContent] = useState(false)
  const [showOverlay, setShowOverlay] = useState(false)
  // 使用共享主题订阅器，避免创建多余的 MutationObserver
  const isDark = useThemeMode()
  const [user, setUser] = useState<User | null>(null)

  // 页面可见性状态 - 用于冻结动态内容更新
  const [isPageVisible, setIsPageVisible] = useState(!document.hidden)
  const pendingUpdatesRef = useRef<Array<(prev: DynamicContent[]) => DynamicContent[]>>([])

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
  const safeSetDynamicContents = useCallback((updater: (prev: DynamicContent[]) => DynamicContent[]) => {
    if (document.hidden) {
      // 页面隐藏时，暂存更新
      pendingUpdatesRef.current.push(updater)
    }
    else {
      // 页面可见时，直接应用更新
      setDynamicContents(updater)
    }
  }, [])

  // 页面恢复可见时，应用所有暂存的更新
  useEffect(() => {
    if (isPageVisible && pendingUpdatesRef.current.length > 0) {
      // 合并所有暂存的更新
      setDynamicContents((prev) => {
        let result = prev
        for (const updater of pendingUpdatesRef.current) {
          result = updater(result)
        }
        return result
      })
      // 清空暂存
      pendingUpdatesRef.current = []
    }
  }, [isPageVisible])

  // 过滤掉空白内容，获取有效的动态内容列表（提前定义，供轮播逻辑使用）
  const validContents = useMemo(() => {
    return dynamicContents.filter((c) => {
      // 必须有图标和文本
      if (!c.icon || !c.text)
        return false
      // 文本不能是空字符串或只有空白
      if (typeof c.text === 'string' && c.text.trim().length === 0)
        return false
      return true
    })
  }, [dynamicContents])

  // 文本引用，用于检测是否需要滚动
  const textRef = useRef<HTMLSpanElement>(null)
  const [needsScroll, setNeedsScroll] = useState(false)

  // 壁纸管理 Hook（替代之前的独立状态和函数）
  const { wallpaperUrl, canRefresh: canRefreshWallpaper, refreshWallpaper, loadWallpaper } = useWallpaper()

  // 音乐播放器 Hook（从 GlobalControlPanel 分离）
  const musicPlayer = useMusicPlayer()

  // 音量弹窗状态（UI相关，保留在这里）
  const [showVolumePopup, setShowVolumePopup] = useState(false)

  // DOM 引用
  const triggerRef = useRef<HTMLDivElement>(null)
  const expandedContentRef = useRef<HTMLDivElement>(null)
  const volumeControlRef = useRef<HTMLDivElement>(null)
  const perf = usePerformanceProfile()
  const anim = useAnimationLevel()
  const { preference: animPreference, togglePerformanceMode } = useAnimationPreference()

  useEffect(() => {
    // 主题状态现在由 useThemeMode() hook 自动管理
    // 认证检查现在由 AuthContext 管理，用户信息会自动同步

    // 加载动态内容
    loadDynamicContents()

    // 加载壁纸配置以初始化 canRefresh 状态
    loadWallpaper()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []) // 只在挂载时运行一次，避免循环依赖

  // 点击外部关闭音量弹窗
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (volumeControlRef.current && !volumeControlRef.current.contains(event.target as Node)) {
        setShowVolumePopup(false)
      }
    }

    if (showVolumePopup) {
      document.addEventListener('mousedown', handleClickOutside)
    }
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
    }
  }, [showVolumePopup])

  // 注意：壁纸颜色提取完全由 AppLayout 负责
  // GlobalControlPanel 不再处理壁纸颜色，只处理音乐封面颜色

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
      if (event.type === 'add' || event.type === 'update' || event.type === 'remove' || event.type === 'clear') {
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
      const builtinContents = prev.filter(c => !c.type.toString().startsWith('tapp-'))

      // 获取所有 Tapp 内容
      const tappContents = dynamicContentProvider.getAllContents()
        .filter(c => c.type.toString().startsWith('tapp-'))
        .map(c => ({
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
  const getWeatherText = useCallback((code: number): string => {
    const weatherT = t.weather ?? {}
    if (code === 0 || code === 1)
      return weatherT.sunny ?? 'Sunny'
    if (code === 2 || code === 3)
      return weatherT.cloudy ?? 'Cloudy'
    if (code === 45 || code === 48)
      return weatherT.foggy ?? 'Foggy'
    if (code >= 51 && code <= 67)
      return weatherT.rainy ?? 'Rainy'
    if (code >= 80 && code <= 82)
      return weatherT.rainy ?? 'Rainy'
    if (code >= 71 && code <= 77)
      return weatherT.snowy ?? 'Snowy'
    if (code >= 85 && code <= 86)
      return weatherT.snowy ?? 'Snowy'
    if (code >= 95 && code <= 99)
      return weatherT.thunderstorm ?? 'Thunderstorm'
    return weatherT.unavailable ?? 'Unknown'
  }, [t.weather])

  // 加载动态内容
  const loadDynamicContents = useCallback(async () => {
    const contents: DynamicContent[] = []

    // 1. 问候语（始终显示，立即加载）
    const greetingTranslations = {
      morning: t.greeting?.morning ?? 'Good morning',
      noon: t.greeting?.noon ?? 'Good afternoon',
      afternoon: t.greeting?.afternoon ?? 'Good afternoon',
      evening: t.greeting?.evening ?? 'Good evening',
      night: t.greeting?.night ?? 'Good night',
    }
    const greeting = getGreeting(user?.username, greetingTranslations, locale)
    contents.push({
      type: 'greeting',
      icon: greeting.icon,
      text: greeting.text || greetingTranslations.afternoon,
      subtext: greeting.time,
    })

    // 立即显示问候语（保留音乐和 Tapp 内容，只更新内置内容）
    safeSetDynamicContents((prev) => {
      // 保留音乐和 Tapp 类型的内容
      const preserved = prev.filter(c => c.type === 'music' || c.type.toString().startsWith('tapp-'))
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
        const filtered = prev.filter(c => c.type !== 'weather')
        // 在问候语后插入天气信息
        const greetingIndex = filtered.findIndex(c => c.type === 'greeting')
        const insertIndex = greetingIndex >= 0 ? greetingIndex + 1 : 0
        filtered.splice(insertIndex, 0, {
          type: 'weather',
          icon: weatherData.icon,
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
    }
    else {
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
              const filtered = prev.filter(c => c.type !== 'weather')
              // 在问候语后插入天气信息
              const greetingIndex = filtered.findIndex(c => c.type === 'greeting')
              const insertIndex = greetingIndex >= 0 ? greetingIndex + 1 : 0
              filtered.splice(insertIndex, 0, {
                type: 'weather',
                icon: weather.icon,
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
        }
        catch (error) {
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
        const filtered = prev.filter(c => c.type !== 'quote')
        return [...filtered, {
          type: 'quote',
          icon: '💭',
          text: quoteData.text,
          subtext: quoteData.author || undefined,
          showSubtext: false,
        }]
      })

      // 同步到动态内容提供者
      dynamicContentProvider.setContent('builtin', {
        type: 'quote',
        icon: '💭',
        text: quoteData.text,
        subtext: quoteData.author || undefined,
        priority: 50,
        showSubtext: false,
      })
    }
    else {
      // 首次加载一言数据
      loadResource.high('quote-info', async () => {
        try {
          const quote = await getRandomQuote(locale)
          if (quote && quote.text) {
            setQuoteData(quote)
            safeSetDynamicContents((prev) => {
              // 移除旧的一言内容（如果有）
              const filtered = prev.filter(c => c.type !== 'quote')
              return [...filtered, {
                type: 'quote',
                icon: '💭',
                text: quote.text,
                subtext: quote.author || undefined,
                showSubtext: false,
              }]
            })

            // 同步到动态内容提供者
            dynamicContentProvider.setContent('builtin', {
              type: 'quote',
              icon: '💭',
              text: quote.text,
              subtext: quote.author || undefined,
              priority: 50,
              showSubtext: false,
            })
          }
        }
        catch (error) {
          // 静默处理错误 - 一言不可用时不显示
          console.debug('[GlobalControlPanel] Quote unavailable:', error)
        }
      })
    }

    // 4. 加载 Tapp 提供的动态内容
    refreshTappContents()
  }, [user?.username, t, locale, dynamicContentProvider, refreshTappContents, safeSetDynamicContents, weatherData, quoteData, getWeatherText])

  // 同步 AuthContext 的用户信息到本地状态
  useEffect(() => {
    if (authUser) {
      setUser(authUser as User)
    }
    else {
      setUser(null)
    }
  }, [authUser])

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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []) // 仅在组件挂载时运行一次

  // 确保 currentContentIndex 在有效范围内（使用 validContents）
  useEffect(() => {
    if (validContents.length > 0 && currentContentIndex >= validContents.length) {
      setCurrentContentIndex(0)
    }
  }, [validContents.length, currentContentIndex])

  // 动态内容轮播（带淡入淡出效果）- 仅在有有效内容时运行
  useEffect(() => {
    // 在以下情况禁用轮播：展开面板 / 悬停 / 有效内容为空 / 页面隐藏
    if (validContents.length === 0 || isExpanded || isHovering)
      return

    let timerId: number | null = null
    let cancelled = false

    const cycle = () => {
      if (cancelled || document.hidden)
        return
      setIsTransitioning(true)
      timerId = window.setTimeout(() => {
        if (cancelled)
          return
        setCurrentContentIndex(prev => (prev + 1) % validContents.length)
        // 稍等一帧后开始淡入，确保内容已更新
        window.setTimeout(() => setIsTransitioning(false), 80)
        // 下一次循环：延长停留时间到 15秒，低端设备 30秒
        const base = 15000
        const nextDelay = Math.round(base * (anim.durationScale || 1))
        timerId = window.setTimeout(cycle, nextDelay)
      }, 300)
    }

    // 首次延迟启动，等待 6 秒后开始轮播
    const startDelay = Math.round(6000 * (anim.durationScale || 1))
    timerId = window.setTimeout(cycle, startDelay)

    const handleVisibility = () => {
      if (document.hidden) {
        // 页面隐藏时清除定时器
        if (timerId) {
          clearTimeout(timerId)
          timerId = null
        }
      }
      else if (!cancelled) {
        // 页面重新可见时重新启动轮播
        if (timerId)
          clearTimeout(timerId)
        const restartDelay = Math.round(2000 * (anim.durationScale || 1))
        timerId = window.setTimeout(cycle, restartDelay)
      }
    }
    document.addEventListener('visibilitychange', handleVisibility)

    return () => {
      cancelled = true
      if (timerId)
        clearTimeout(timerId)
      document.removeEventListener('visibilitychange', handleVisibility)
    }
  }, [validContents.length, isExpanded, isHovering, anim.durationScale])

  // 主题变化已通过 useThemeMode() hook 自动响应
  // 无需额外的 MutationObserver

  // 动态计算展开面板的高度 - 🔧 事件驱动，无轮询
  // 外部可通过 dispatchEvent(new CustomEvent('gcp-remeasure')) 触发重测
  useLayoutEffect(() => {
    if (!triggerRef.current)
      return
    const triggerEl = triggerRef.current

    if (!isExpanded) {
      triggerEl.style.height = '3rem'
      return
    }
    if (!expandedContentRef.current)
      return
    const contentEl = expandedContentRef.current

    let lastHeight = 0
    let lastUpdateTime = 0
    let pendingMeasure = false
    let measureTimeout: number | null = null
    let isAnimating = false // 🔧 动画状态标记

    // ⚠️ 移动端检测 - 使用性能配置而非 window.innerWidth，更可靠
    const isMobileDevice = perf.isMobile || perf.lowEndDevice

    // ⚠️ 节流时间：防止短时间内多次事件触发重复测量
    // 🔧 加大节流时间，减少克隆测量频率
    const THROTTLE_MS = isMobileDevice ? 1200 : 600

    const measure = (force = false) => {
      // 🔧 动画期间跳过测量（除非强制）
      if (isAnimating && !force)
        return

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

      // 计算目标宽度用于测量（避免动画过程中的宽度变化导致高度计算错误）
      const isMobile = window.innerWidth <= 640
      // Desktop: 400px - padding(1.375rem * 2 = 44px) = 356px
      // Mobile: (100vw - 1.5rem) - padding(1rem * 2 = 32px) = 100vw - 56px
      const targetWidth = isMobile
        ? window.innerWidth - 56
        : 356

      // 🔧 优化：使用轻量级测量方式
      // 对于播放列表视图，使用估算高度而非完整克隆
      const playlistScroll = contentEl.querySelector('.music-playlist-scroll')
      let raw: number

      if (playlistScroll && playlistScroll.children.length > 15) {
        // 🔧 播放列表超过 15 项时，使用估算而非克隆
        // 估算：头部约 40px，每项约 52px，底部边距约 16px
        const playlistHeader = contentEl.querySelector('.music-playlist-header')
        const headerHeight = playlistHeader?.getBoundingClientRect().height ?? 40
        const itemCount = Math.min(playlistScroll.children.length, 8) // 最多显示 8 项
        const estimatedPlaylistHeight = headerHeight + (itemCount * 52) + 16

        // 测量除播放列表外的其他内容
        const otherContent = contentEl.cloneNode(true) as HTMLElement
        const clonedPlaylist = otherContent.querySelector('.music-view-playlist')
        if (clonedPlaylist) {
          (clonedPlaylist as HTMLElement).style.height = `${estimatedPlaylistHeight}px`
          const clonedScroll = clonedPlaylist.querySelector('.music-playlist-scroll')
          if (clonedScroll) {
            clonedScroll.innerHTML = '' // 清空列表项
          }
        }

        otherContent.style.position = 'absolute'
        otherContent.style.visibility = 'hidden'
        otherContent.style.height = 'auto'
        otherContent.style.width = `${targetWidth}px`

        document.body.appendChild(otherContent)
        raw = otherContent.offsetHeight
        document.body.removeChild(otherContent)
      }
      else {
        // 常规克隆测量
        const clone = contentEl.cloneNode(true) as HTMLElement
        clone.style.position = 'absolute'
        clone.style.visibility = 'hidden'
        clone.style.height = 'auto'
        clone.style.width = `${targetWidth}px`

        document.body.appendChild(clone)
        raw = clone.offsetHeight
        document.body.removeChild(clone)
      }

      // 适当补偿 (考虑内边距 + 过渡)
      const compensated = Math.ceil(raw * 1.08)

      if (Math.abs(compensated - lastHeight) > 4) {
        lastHeight = compensated
        triggerEl.style.height = `${compensated}px`
      }
    }

    // 立即测量，确保动画起始帧即为正确高度
    measure(true)

    // 🔧 监听动画状态
    const handleAnimationStart = () => {
      isAnimating = true
    }
    const handleAnimationEnd = () => {
      isAnimating = false
      // 动画结束后重新测量
      lastUpdateTime = 0
      measure(true)
    }
    window.addEventListener('gcp-animation-start', handleAnimationStart)
    window.addEventListener('gcp-animation-end', handleAnimationEnd)

    // 🔧 统一使用事件驱动重测（移除轮询）
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
        setTimeout(() => measure(), 100)
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
      window.removeEventListener('gcp-animation-start', handleAnimationStart)
      window.removeEventListener('gcp-animation-end', handleAnimationEnd)
      window.removeEventListener('gcp-remeasure', handleRemeasure)
      window.removeEventListener('control-panel-content-resize', handleRemeasure)
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
  }, [isExpanded, perf.lowEndDevice, perf.isMobile, anim.level])

  const toggleTheme = useCallback(() => {
    const html = document.documentElement
    const newIsDark = !isDark

    if (newIsDark) {
      html.classList.add('dark')
      html.classList.remove('light')
      localStorage.setItem('theme', 'dark')
    }
    else {
      html.classList.add('light')
      html.classList.remove('dark')
      localStorage.setItem('theme', 'light')
    }
    // isDark 状态由 useThemeMode() hook 自动响应 class 变化，无需手动 setIsDark

    // 更新 meta theme-color - 使用壁纸颜色
    const metaThemeColor = document.querySelector('meta[name="theme-color"]')
    if (metaThemeColor) {
      const primaryColor = getComputedStyle(document.documentElement).getPropertyValue('--color-primary').trim() || '#94a3b8'
      metaThemeColor.setAttribute('content', primaryColor)
    }
  }, [isDark])

  const handleTogglePanel = useCallback(() => {
    // 🔧 通知子组件动画开始
    window.dispatchEvent(new CustomEvent('gcp-animation-start'))

    if (isExpanded) {
      // 收缩：面板内容立即淡出，容器开始收缩，动态内容在中途淡入
      setShowPanelContent(false)
      setShowOverlay(false) // 遮罩层开始淡出
      setIsExpanded(false)
      setTimeout(() => {
        setShowDynamicContent(true)
      }, 400) // 容器收缩到一半时显示（0.7s 动画的中点）
      // 🔧 动画结束后通知
      setTimeout(() => {
        window.dispatchEvent(new CustomEvent('gcp-animation-end'))
      }, 700)
    }
    else {
      // 展开：动态内容立即淡出，容器开始展开，面板内容在中途淡入
      setShowDynamicContent(false)
      setIsExpanded(true)
      // 遮罩层立即显示但透明，然后淡入
      setTimeout(() => {
        setShowOverlay(true)
      }, 0)
      setTimeout(() => {
        setShowPanelContent(true)
      }, 400) // 容器展开到一半时显示（0.7s 动画的中点）
      // 🔧 动画结束后通知
      setTimeout(() => {
        window.dispatchEvent(new CustomEvent('gcp-animation-end'))
      }, 700)
    }
  }, [isExpanded])

  const handleClosePanel = useCallback(() => {
    handleTogglePanel()
  }, [handleTogglePanel])

  // 监听打开控制面板事件（来自音乐小组件等点击）
  useEffect(() => {
    const handleOpenPanel = () => {
      if (!isExpanded) {
        handleTogglePanel()
      }
    }

    window.addEventListener('open-control-panel', handleOpenPanel)
    return () => {
      window.removeEventListener('open-control-panel', handleOpenPanel)
    }
  }, [isExpanded, handleTogglePanel])

  // 当有歌词时，更新动态内容以显示歌词（仅播放时）
  // 使用 useRef 来减少状态更新频率
  const lastLyricTextRef = useRef<string>('')
  const lastSongIdRef = useRef<string>('')
  const lastPlayingStateRef = useRef<boolean>(false)

  useEffect(() => {
    const { currentSong, isPlaying, lyrics, currentLyricIndex } = musicPlayer

    // 早期返回：面板展开时不更新动态内容
    // 注意：页面隐藏时由 safeSetDynamicContents 自动暂存更新
    if (isExpanded)
      return

    if (currentSong && isPlaying && lyrics.length > 0 && currentLyricIndex >= 0) {
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
      }
      else {
        // 最后一句歌词，默认8秒
        lyricDuration = 8
      }

      // 播放时显示歌词 - 使用函数式更新避免闭包问题
      safeSetDynamicContents((prev) => {
        const filtered = prev.filter(c => c.type !== 'music')
        return [
          {
            type: 'music' as const,
            icon: '🎵',
            text: currentLyric.text,
            subtext: `${currentSong.name} - ${currentSong.artist}`,
            lyricDuration, // 传入歌词持续时间
          },
          ...filtered,
        ]
      })
    }
    else if (currentSong) {
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
        const filtered = prev.filter(c => c.type !== 'music')
        return [
          {
            type: 'music' as const,
            icon: isPlaying ? '🎵' : '⏸️',
            text: currentSong.name,
            subtext: currentSong.artist,
          },
          ...filtered,
        ]
      })
    }
    else if (lastSongIdRef.current !== '') {
      // 没有歌曲时移除音乐内容（仅当之前有歌曲时）
      lastLyricTextRef.current = ''
      lastSongIdRef.current = ''
      lastPlayingStateRef.current = false
      safeSetDynamicContents(prev => prev.filter(c => c.type !== 'music'))
    }
  }, [musicPlayer.currentSong?.id, musicPlayer.lyrics.length, musicPlayer.currentLyricIndex, musicPlayer.isPlaying, isExpanded, safeSetDynamicContents])

  // 确保索引在有效范围内
  const safeContentIndex = validContents.length > 0
    ? Math.min(currentContentIndex, validContents.length - 1)
    : 0

  // 获取当前显示的动态内容
  const currentContent = validContents.length > 0 ? validContents[safeContentIndex] : null

  // 用于跟踪上一次歌词文本，实现切换时的淡入淡出
  const prevLyricTextRef = useRef<string>('')
  // 用于强制触发滚动动画重置的key
  const scrollResetKeyRef = useRef<number>(0)

  // 歌词切换时的淡入淡出效果（独立处理，不影响滚动检测）
  useEffect(() => {
    if (!textRef.current || !currentContent)
      return

    const element = textRef.current
    const isMusic = currentContent.type === 'music'
    const textChanged = isMusic && prevLyricTextRef.current !== '' && prevLyricTextRef.current !== currentContent.text

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
    }
    else {
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

          // 🎵 歌词特殊处理：使用精确的时间轴同步
          if (currentContent.type === 'music' && currentContent.lyricDuration) {
            // 歌词：使用歌词持续时间（到下一句的时间差）
            // 减去0.5秒作为缓冲，留出0.3秒作为延迟，确保流畅过渡
            duration = Math.max(1.5, currentContent.lyricDuration - 0.5)
            delay = '0.3s' // 歌词用更短的延迟，快速响应
          }
          else if (currentContent.type === 'music') {
            // 没有时间轴的歌词（最后一句或无时间戳），默认 4 秒
            duration = 4
            delay = '0.5s'
          }
          else {
            // 普通内容：根据溢出量动态计算，每20px需要1秒，最短10秒，最长20秒
            duration = Math.max(10, Math.min(20, Math.ceil(overflowAmount / 20) + 10))
            delay = '1.5s'
          }

          element.style.setProperty('--scroll-duration', `${duration}s`)
          element.style.setProperty('--scroll-delay', delay)

          // 重置滚动动画（确保从头开始）
          setNeedsScroll(false)
          requestAnimationFrame(() => {
            setNeedsScroll(true)
          })
        }
        else {
          element.style.removeProperty('--scroll-distance')
          element.style.removeProperty('--scroll-duration')
          element.style.removeProperty('--scroll-delay')
          setNeedsScroll(false)
        }
      })
    }

    // 使用统一调度器的ResizeObserver，共享Observer实例，性能更优
    const unobserve = observeResize(element, (_entry) => {
      updateScrollAnimation()
    }, { immediate: true }) // 立即执行首次测量

    // 监听内容变化，强制重新计算滚动
    // 这确保歌词切换时滚动动画会重置
    updateScrollAnimation()

    return () => {
      unobserve()
    }
  }, [currentContent?.text, currentContent?.type, currentContent?.lyricDuration, currentContentIndex, scrollResetKeyRef.current])

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

  return (
    <React.Fragment>
      {/* 顶部控制栏 - 智能岛 */}
      <div className="global-control-bar">
        <div className="control-bar-content">
          <div
            ref={triggerRef}
            className={`control-bar-trigger ${isExpanded ? 'expanded' : ''}`}
            onMouseEnter={() => setIsHovering(true)}
            onMouseLeave={() => setIsHovering(false)}
          >
            {/* 动态轮播内容 - 仅在有有效内容时显示 */}
            {hasValidContent && currentContent && (
              <div
                className={`dynamic-content-wrapper ${!showDynamicContent || isTransitioning ? 'hidden' : ''}`}
                onClick={handleTogglePanel}
              >
                <span className="dynamic-icon">
                  {currentContent.icon}
                </span>
                <div className="dynamic-text">
                  <span
                    ref={textRef}
                    className={`dynamic-text-main ${needsScroll ? 'scrolling' : ''}`}
                  >
                    {currentContent.text}
                  </span>
                  {/* 根据 showSubtext 属性或类型判断是否显示副文本 */}
                  {currentContent.subtext && shouldShowSubtext(currentContent) && (
                    <span className="dynamic-text-sub">{currentContent.subtext}</span>
                  )}
                </div>
                <svg className="dynamic-arrow" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                </svg>
              </div>
            )}

            {/* 无有效内容时，仍需保持可点击区域以展开面板 */}
            {!hasValidContent && !isExpanded && (
              <div
                className={`dynamic-content-wrapper empty-state ${!showDynamicContent ? 'hidden' : ''}`}
                onClick={handleTogglePanel}
              >
                <svg className="dynamic-arrow" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                </svg>
              </div>
            )}

            {/* 展开的控制面板内容 - 通过 JS 控制显示/隐藏 */}
            <div ref={expandedContentRef} className={`expanded-panel-content ${showPanelContent ? 'visible' : ''}`}>
              {/* 头部 - 用户信息按钮 */}
              <div className="control-panel-header">
                <UserSection onClosePanel={handleClosePanel} />
                <button
                  onClick={handleClosePanel}
                  className="control-close-btn"
                  aria-label={t.common.close}
                >
                  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>

              {/* 动态信息卡片 - 切换显示 */}
              <ControlPanelWidgets isAdmin={user?.is_admin} />

              {/* 音乐播放器 */}
              <MusicPlayer player={musicPlayer} />

              {/* 控制项网格 - 一行两个 */}
              <div className="control-items-grid">
                {/* 主题切换 */}
                <div className="control-item control-item-compact">
                  <div className="control-item-info">
                    <div className="control-item-icon icon-theme">
                      {isDark ? '🌙' : '☀️'}
                    </div>
                    <div>
                      <h4 className="control-item-title">{t.controlPanel.appearance}</h4>
                      <p className="control-item-desc">{isDark ? t.controlPanel.dark : t.controlPanel.light}</p>
                    </div>
                  </div>
                  <button
                    onClick={toggleTheme}
                    className={`control-toggle ${isDark ? 'active' : ''}`}
                    aria-label={t.controlPanel.themeSwitch}
                  >
                    <span className="control-toggle-slider"></span>
                  </button>
                </div>

                {/* 动效等级切换 */}
                <div className="control-item control-item-compact">
                  <div className="control-item-info">
                    <div className="control-item-icon icon-performance">
                      {animPreference === 'light' ? '🐌' : animPreference === 'standard' ? '⚡' : '🔄'}
                    </div>
                    <div>
                      <h4 className="control-item-title">{t.controlPanel.animation}</h4>
                      <p className="control-item-desc">
                        {animPreference === 'auto'
                          ? (anim.level === 'light' ? t.controlPanel.lowPerformance : anim.level === 'standard' ? t.controlPanel.highPerformance : t.controlPanel.noAnimation)
                          : animPreference === 'light' ? t.controlPanel.lowPerformance : t.controlPanel.highPerformance}
                      </p>
                    </div>
                  </div>
                  <button
                    onClick={togglePerformanceMode}
                    className={`control-toggle ${(animPreference === 'standard' || (animPreference === 'auto' && anim.level === 'standard')) ? 'active' : ''}`}
                    aria-label={t.controlPanel.animation}
                  >
                    <span className="control-toggle-slider"></span>
                  </button>
                </div>

                {/* 语言切换 */}
                <div className="control-item control-item-compact">
                  <div className="control-item-info">
                    <div className="control-item-icon icon-language">
                      🌐
                    </div>
                    <div>
                      <h4 className="control-item-title">{t.controlPanel.language}</h4>
                      <p className="control-item-desc">{locale === 'zh-CN' ? '简体中文' : locale === 'ja-JP' ? '日本語' : 'English'}</p>
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
                      const nextIndex = e.deltaY > 0
                        ? (currentIndex + 1) % locales.length
                        : (currentIndex - 1 + locales.length) % locales.length
                      setLocale(locales[nextIndex])
                    }}
                    className="language-switch-btn"
                    aria-label={t.controlPanel.languageSwitch}
                  >
                    <span className="language-code">{locale === 'zh-CN' ? '中' : locale === 'ja-JP' ? '日' : 'En'}</span>
                  </button>
                </div>

                {/* 壁纸切换 - 仅在非单一图片链接时显示 */}
                {/* Debug: canRefreshWallpaper = {String(canRefreshWallpaper)} */}
                {canRefreshWallpaper && (
                  <div className="control-item control-item-compact">
                    <div className="control-item-info">
                      <div className="control-item-icon icon-wallpaper">
                        🖼️
                      </div>
                      <div>
                        <h4 className="control-item-title">{t.controlPanel.wallpaper}</h4>
                        <p className="control-item-desc">{t.controlPanel.random}</p>
                      </div>
                    </div>
                    <button
                      onClick={refreshWallpaper}
                      className="control-action-btn"
                      aria-label={t.controlPanel.wallpaperSwitch}
                    >
                      <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                      </svg>
                    </button>
                  </div>
                )}

                {/* 系统配置 - 仅管理员可见 */}
                {user?.is_admin && (
                  <div className="control-item control-item-compact">
                    <div className="control-item-info">
                      <div className="control-item-icon icon-config">
                        ⚙️
                      </div>
                      <div>
                        <h4 className="control-item-title">{t.controlPanel.configuration}</h4>
                        <p className="control-item-desc">{t.controlPanel.system}</p>
                      </div>
                    </div>
                    <button
                      onClick={() => {
                        handleClosePanel()
                        navigate('/config')
                      }}
                      className="control-action-btn"
                      aria-label={t.controlPanel.configuration}
                    >
                      <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 7l5 5m0 0l-5 5m5-5H6" />
                      </svg>
                    </button>
                  </div>
                )}

              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 遮罩层 - 始终存在，通过 CSS 控制显示 */}
      <div
        className={`control-panel-overlay ${showOverlay ? 'visible' : ''}`}
        onClick={handleClosePanel}
      />
    </React.Fragment>
  )
}

export default GlobalControlPanel
