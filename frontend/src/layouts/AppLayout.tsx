/**
 * React 版主布局组件
 * 包含导航栏、背景、全局控制面板
 */

import { useCallback, useEffect, useRef, useState } from 'react'

import { useLocation } from 'react-router-dom'
import GlobalControlPanel from '../components/GlobalControlPanel'
import NavigationIsland from '../components/NavigationIsland'
import { SiteFooter } from '../components/SiteFooter'
import { SocialNetworkSettingsModal } from '../components/widgets/SocialNetworkWidget'

import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { useNotification } from '../contexts/NotificationContext'
import { useIdleEffect, useVisibilityInterval } from '../hooks/animation/atomicHooks'
import { useAnimationLevel } from '../hooks/useAnimationLevel'
import { useEvocativeWallpaper } from '../hooks/useEvocativeWallpaper'
import { useNavAutoHide } from '../hooks/useNavAutoHide'
import { useScrollOptimization } from '../hooks/useScrollOptimization'
import { useSystemSetupCheck } from '../hooks/useSystemSetupCheck'
import { useWallpaper } from '../hooks/useWallpaper'
import { applyColorPalette, extractColorsFromImage } from '../utils/colorExtractor'
import { startFpsMonitor, stopFpsMonitor } from '../utils/performance'
import {
  getColorFromCache,
  saveColorToCache,
  shouldApplyColorExtraction,
} from '../utils/wallpaperColorCache'
import { wallpaperState } from '../utils/wallpaperState'

interface AppLayoutProps {
  children: React.ReactNode
}

export function AppLayout({ children }: AppLayoutProps) {
  const location = useLocation()
  const { t } = useI18n()
  const [backendConnected, setBackendConnected] = useState<boolean | null>(null)
  const [hasEverConnected, setHasEverConnected] = useState(false)
  const { notifications } = useNotification()

  // ℹ️ 性能优化: 移动端/低端设备禁用背景动画
  const anim = useAnimationLevel()

  // 🔧 帧率优化：启用滚动优化和 FPS 监控
  useScrollOptimization({ enabled: true })
  useSystemSetupCheck()

  // 启动/停止 FPS 监控
  useEffect(() => {
    startFpsMonitor()
    return () => stopFpsMonitor()
  }, [])

  // 壁纸管理 Hook
  const { loadWallpaper: loadWallpaperFromHook } = useWallpaper()

  // Evocative 壁纸动效配置状态
  const [evocativeParallax, setEvocativeParallax] = useState(true)
  const [evocativeDynamicBlur, setEvocativeDynamicBlur] = useState(false)
  const [evocativeRipple, setEvocativeRipple] = useState(false)
  const [evocativeFps, setEvocativeFps] = useState(30)
  const [evocativeRippleQuality, setEvocativeRippleQuality] = useState(0.85)
  // 壁纸模糊度状态
  const [wallpaperBlur, setWallpaperBlur] = useState(3)

  // 🎨 Evocative 壁纸动效统一 Hook
  // ⚠️ 低性能模式下强制禁用所有动效
  const isLowPerformance = anim.level === 'light' || anim.level === 'none'
  useEvocativeWallpaper('wallpaper', {
    parallax: {
      enabled: evocativeParallax && !isLowPerformance,
      enableGyroscope: true,
      enableMouse: true,
      maxOffset: 8,
      scale: 1.02,
    },
    dynamicBlur: {
      enabled: evocativeDynamicBlur && !isLowPerformance,
      baseBlur: wallpaperBlur,
      unblurZone: 0.4,
      blurZone: 0.6,
    },
    ripple: {
      enabled: evocativeRipple && !isLowPerformance,
    },
    fps: evocativeFps,
    rippleQuality: evocativeRippleQuality,
  })

  // 🎨 壁纸颜色提取 —— 缓存 → 验证 → 提取 → 应用
  const extractAndApplyColors = useCallback(async (url: string) => {
    if (!wallpaperState.isUrlActive(url))
      return

    // 先检查缓存
    const cachedColors = getColorFromCache(url)
    if (cachedColors) {
      if (wallpaperState.isUrlActive(url))
        applyColorPalette(cachedColors)
      return
    }

    // 检查是否为有效壁纸（包含一致性验证）
    const checkResult = await shouldApplyColorExtraction(url)
    if (!checkResult.shouldApply)
      return

    try {
      const colors = await extractColorsFromImage(url, { context: 'wallpaper' })
      if (wallpaperState.isUrlActive(url)) {
        applyColorPalette(colors)
        saveColorToCache(url, colors)
      }
    }
    catch (error) {
      console.error('颜色提取失败:', error)
    }
  }, [])

  // 加载壁纸和颜色（使用 Hook）
  const loadWallpaper = useCallback(async () => {
    const wallpaperResult = await loadWallpaperFromHook()
    if (!wallpaperResult)
      return

    // 更新 Evocative 动效配置
    if (wallpaperResult.evocative) {
      setEvocativeParallax(wallpaperResult.evocative.parallax)
      setEvocativeDynamicBlur(wallpaperResult.evocative.dynamicBlur)
      setEvocativeRipple(wallpaperResult.evocative.ripple)
      setEvocativeFps(wallpaperResult.evocative.fps)
      setEvocativeRippleQuality(wallpaperResult.evocative.rippleQuality)
    }
    else {
      setEvocativeParallax(wallpaperResult.parallaxEnabled)
    }
    setWallpaperBlur(wallpaperResult.blur)

    await extractAndApplyColors(wallpaperResult.actualUrl)
  }, [loadWallpaperFromHook, extractAndApplyColors])

  // 检查后端连接状态 - 使用 useIdleInterval 降低主线程占用
  const checkBackendRef = useRef<() => Promise<void>>(undefined)
  checkBackendRef.current = async () => {
    try {
      const response = await fetch(`${API_URL}/health`, {
        method: 'GET',
        signal: AbortSignal.timeout(5000), // 5秒超时
      })
      setBackendConnected(response.ok)
      if (response.ok) {
        setHasEverConnected(true)
      }
    }
    catch {
      setBackendConnected(false)
    }
  }

  // 首次检查延迟到主线程空闲时执行
  useIdleEffect(() => {
    checkBackendRef.current?.()
  }, [], { priority: 'high' })

  // 每30秒检查一次，页面隐藏时自动暂停
  useVisibilityInterval(() => {
    checkBackendRef.current?.()
  }, { delay: 30000, enabled: true })

  // 初始化：加载壁纸（仅首次挂载执行）
  const hasInitializedRef = useRef(false)
  useEffect(() => {
    // 防止重复初始化
    if (hasInitializedRef.current)
      return
    hasInitializedRef.current = true

    console.debug('[AppLayout] Initializing wallpaper load...');
    (async () => {
      try {
        await loadWallpaper()
        console.debug('[AppLayout] Wallpaper load completed')
      }
      catch (error) {
        console.error('[AppLayout] Wallpaper load failed:', error)
      }
    })()
    // 认证检查现在由 AuthContext 管理，按需触发
  }, [])

  // 监听壁纸变化事件（由 GlobalControlPanel 触发）
  useEffect(() => {
    const handleWallpaperChanged = async (e: Event) => {
      const newUrl = (e as CustomEvent).detail?.url
      if (newUrl)
        await extractAndApplyColors(newUrl)
    }

    window.addEventListener('wallpaperChanged', handleWallpaperChanged)
    return () => {
      window.removeEventListener('wallpaperChanged', handleWallpaperChanged)
    }
  }, [])

  // 导航岛自动隐藏
  useNavAutoHide()

  return (
    <>
      {/* 全局控制面板 */}
      <div id="global-control-panel-root">
        <GlobalControlPanel />
      </div>

      {/* 背景 */}
      <div id="bg-container" className="fixed inset-0 -z-10 overflow-hidden">
        <div id="wallpaper" className="absolute inset-0 bg-cover bg-center bg-no-repeat transition-opacity duration-700 ease-in-out"></div>
        <div id="bg-gradient" className="absolute inset-0 bg-gradient-to-b from-transparent from-[35%] via-white/40 via-[55%] to-white/90 to-[85%] transition-opacity duration-500 ease-out"></div>
        {/* ⚠️ 性能优化: 只在标准设备上渲染动画背景
            🔥 使用 GPU 加速的独立合成层，避免 mix-blend-mode 导致的 CPU 回退 */}
        {anim.level === 'standard' && (
          <div className="absolute inset-0 opacity-20 transition-opacity duration-700 bg-animation-container">
            {/* 🔥 移除 mix-blend-multiply，改用 opacity 叠加，确保 GPU 合成 */}
            <div className="absolute top-[40%] left-10 w-96 h-96 bg-green-400/40 rounded-full filter blur-3xl animate-blob-fast bg-blob-element" />
            <div className="absolute top-[40%] right-10 w-96 h-96 bg-pink-400/40 rounded-full filter blur-3xl animate-blob-fast animation-delay-2000 bg-blob-element" />
            <div className="absolute top-[60%] left-1/2 -translate-x-1/2 w-96 h-96 bg-blue-400/35 rounded-full filter blur-3xl animate-blob-fast animation-delay-4000 bg-blob-element" />
          </div>
        )}
        <div className="absolute inset-0 bg-grid-pattern opacity-[0.02]"></div>
      </div>

      {/* 导航栏 - 使用新的 NavigationIsland 组件 */}
      <NavigationIsland />

      {/*
        屏幕角落提示容器 - 统一管理所有固定提示，确保不重叠

        使用说明：
        1. 所有需要显示在屏幕角落的提示都应该添加到这个容器内
        2. 容器使用 flex-col gap-3 自动堆叠提示
        3. 父容器 pointer-events-none，子元素需要 pointer-events-auto
        4. 响应式定位已配置好，自动避开导航岛
      */}
      <div className="fixed z-[100] pointer-events-none
        bottom-6 left-6
        md:bottom-6 md:left-[7.5rem]
        flex flex-col gap-3 max-w-xs"
      >

        {/* 后端未连接提示 - 只在曾经连接过但现在断开时显示 */}
        {backendConnected === false && hasEverConnected && (
          <div className="pointer-events-auto animate-fade-in">
            <div className="glass rounded-xl px-4 py-3 shadow-lg border border-red-200/50 dark:border-red-800/50 bg-red-50/80 dark:bg-red-950/80 backdrop-blur-md">
              <div className="flex items-center gap-3">
                <div className="flex-shrink-0">
                  <div className="w-2 h-2 bg-red-500 rounded-full animate-pulse"></div>
                </div>
                <div>
                  <p className="text-sm font-medium text-red-900 dark:text-red-100">{t.setup.backendDisconnected}</p>
                  <p className="text-xs text-red-700 dark:text-red-300 mt-0.5">{t.setup.reconnecting}</p>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* 全局通知 - 从 NotificationContext 渲染 */}
        {notifications.map(notification => (
          <div key={notification.id} className="pointer-events-auto animate-fade-in">
            <div className={`glass rounded-xl px-4 py-3 shadow-lg border backdrop-blur-md ${
              notification.type === 'loading'
                ? 'border-gray-200/50 dark:border-neutral-700/50'
                : notification.type === 'error'
                  ? 'border-red-200/50 dark:border-red-800/50 bg-red-50/80 dark:bg-red-950/80'
                  : 'border-blue-200/50 dark:border-blue-800/50 bg-blue-50/80 dark:bg-blue-950/80'
            }`}
            >
              <div className="flex items-center gap-3">
                {notification.type === 'loading' && (
                  <div className="w-4 h-4 rounded-full bg-gradient-radial from-indigo-400/30 to-transparent animate-pulse"></div>
                )}
                {notification.type === 'error' && (
                  <div className="flex-shrink-0">
                    <div className="w-2 h-2 bg-red-500 rounded-full"></div>
                  </div>
                )}
                {notification.type === 'info' && (
                  <div className="flex-shrink-0">
                    <div className="w-2 h-2 bg-blue-500 rounded-full"></div>
                  </div>
                )}
                <span className={`text-sm font-medium ${
                  notification.type === 'error'
                    ? 'text-red-900 dark:text-red-100'
                    : notification.type === 'info'
                      ? 'text-blue-900 dark:text-blue-100'
                      : 'text-gray-700 dark:text-gray-200'
                }`}
                >
                  {notification.message}
                </span>
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* 主内容区域 */}
      <main className="relative z-10">
        {children}
      </main>

      {/* 全局设置弹窗 - 整个应用只渲染一次 */}
      <SocialNetworkSettingsModal />

      {/* 站点底部信息 */}
      <SiteFooter isHomePage={location.pathname === '/'} />
    </>
  )
}

export default AppLayout
