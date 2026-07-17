/**
 * React 版主布局组件
 * 包含导航栏、背景、全局控制面板
 */

import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'

import { useLocation } from 'react-router-dom'
import GlobalControlPanel from '../components/GlobalControlPanel'
import NavigationIsland from '../components/NavigationIsland'
import { SiteFooter } from '../components/SiteFooter'
import { SurfaceThemeApplier } from '../components/SurfaceThemeApplier'
import { ToastContainer } from '../components/ToastContainer'

import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import {
  useIdleEffect,
  useVisibilityInterval,
} from '../hooks/animation/atomicHooks'
import { useAnimationLevel } from '../hooks/useAnimationLevel'
import { useEvocativeWallpaper } from '../hooks/useEvocativeWallpaper'
import { useNavAutoHide } from '../hooks/useNavAutoHide'
import { useScrollOptimization } from '../hooks/useScrollOptimization'
import { useSystemSetupCheck } from '../hooks/useSystemSetupCheck'
import { useWallpaper } from '../hooks/useWallpaper'
import {
  applyColorPalette,
  extractColorsFromImage,
} from '../utils/colorExtractor'
import { startFpsMonitor, stopFpsMonitor } from '../utils/performance'
import {
  getColorFromCache,
  saveColorToCache,
  shouldApplyColorExtraction,
} from '../utils/wallpaperColorCache'
import { wallpaperState } from '../utils/wallpaperState'
import './AppLayout.css'

// 懒加载设置弹窗 — 1769 行的 SocialNetworkWidget 延迟到需要时才加载
const SocialNetworkSettingsModal = lazy(() =>
  import('../components/widgets/SocialNetworkWidget').then((m) => ({
    default: m.SocialNetworkSettingsModal,
  })),
)
const ReportCardSettingsModal = lazy(() =>
  import('../components/widgets/ReportCardWidget').then((m) => ({
    default: m.ReportCardSettingsModal,
  })),
)
const GamePresenceSettingsModal = lazy(() =>
  import('../components/widgets/GamePresenceWidget').then((m) => ({
    default: m.GamePresenceSettingsModal,
  })),
)

interface AppLayoutProps {
  children: React.ReactNode
}

export function AppLayout({ children }: AppLayoutProps) {
  const location = useLocation()
  const { t } = useI18n()
  const [backendConnected, setBackendConnected] = useState<boolean | null>(null)
  const [hasEverConnected, setHasEverConnected] = useState(false)
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

  // Evocative 壁纸动效配置状态（合并为单一对象，减少 hook 开销）
  const [evocativeConfig, setEvocativeConfig] = useState({
    parallax: true,
    dynamicBlur: false,
    ripple: false,
    fps: 30,
    rippleQuality: 0.85,
    blur: 3,
  })

  // 🎨 Evocative 壁纸动效统一 Hook
  // ⚠️ 低性能模式下强制禁用所有动效
  const isLowPerformance = anim.level === 'light' || anim.level === 'none'
  useEvocativeWallpaper('wallpaper', {
    parallax: {
      enabled: evocativeConfig.parallax && !isLowPerformance,
      enableGyroscope: true,
      enableMouse: true,
      maxOffset: 8,
      scale: 1.02,
    },
    dynamicBlur: {
      enabled: evocativeConfig.dynamicBlur && !isLowPerformance,
      baseBlur: evocativeConfig.blur,
      unblurZone: 0.4,
      blurZone: 0.6,
    },
    ripple: {
      enabled: evocativeConfig.ripple && !isLowPerformance,
    },
    fps: evocativeConfig.fps,
    rippleQuality: evocativeConfig.rippleQuality,
  })

  // 🎨 壁纸颜色提取 —— 缓存 → 验证 → 提取 → 应用
  const extractAndApplyColors = useCallback(async (url: string) => {
    if (!wallpaperState.isUrlActive(url)) return

    // 先检查缓存
    const cachedColors = getColorFromCache(url)
    if (cachedColors) {
      if (wallpaperState.isUrlActive(url)) applyColorPalette(cachedColors)
      return
    }

    // 检查是否为有效壁纸（包含一致性验证）
    const checkResult = await shouldApplyColorExtraction(url)
    if (!checkResult.shouldApply) return

    try {
      const colors = await extractColorsFromImage(url, {
        context: 'wallpaper',
      })
      if (wallpaperState.isUrlActive(url)) {
        applyColorPalette(colors)
        saveColorToCache(url, colors)
      }
    } catch (error) {
      console.error('颜色提取失败:', error)
    }
  }, [])

  // 加载壁纸和颜色（使用 Hook）
  const loadWallpaper = useCallback(async () => {
    const wallpaperResult = await loadWallpaperFromHook()
    if (!wallpaperResult) return

    // 更新 Evocative 动效配置（单次 setState 替代 6 次）
    if (wallpaperResult.evocative) {
      setEvocativeConfig({
        parallax: wallpaperResult.evocative.parallax,
        dynamicBlur: wallpaperResult.evocative.dynamicBlur,
        ripple: wallpaperResult.evocative.ripple,
        fps: wallpaperResult.evocative.fps,
        rippleQuality: wallpaperResult.evocative.rippleQuality,
        blur: wallpaperResult.blur,
      })
    } else {
      setEvocativeConfig((prev) => ({
        ...prev,
        parallax: wallpaperResult.evocative.parallax,
        blur: wallpaperResult.blur,
      }))
    }

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
    } catch {
      setBackendConnected(false)
    }
  }

  // 首次检查延迟到主线程空闲时执行
  useIdleEffect(
    () => {
      checkBackendRef.current?.()
    },
    [],
    { priority: 'high' },
  )

  // 每30秒检查一次，页面隐藏时自动暂停
  useVisibilityInterval(
    () => {
      checkBackendRef.current?.()
    },
    { delay: 30000, enabled: true },
  )

  // 初始化：加载壁纸（仅首次挂载执行）
  const hasInitializedRef = useRef(false)
  useEffect(() => {
    // 防止重复初始化
    if (hasInitializedRef.current) return
    hasInitializedRef.current = true

    console.debug('[AppLayout] Initializing wallpaper load...')
    ;(async () => {
      try {
        await loadWallpaper()
        console.debug('[AppLayout] Wallpaper load completed')
      } catch (error) {
        console.error('[AppLayout] Wallpaper load failed:', error)
      }
    })()
    // 认证检查现在由 AuthContext 管理，按需触发
  }, [])

  // 监听壁纸变化事件（由 GlobalControlPanel 触发）
  useEffect(() => {
    const handleWallpaperChanged = async (e: Event) => {
      const newUrl = (e as CustomEvent).detail?.url
      if (newUrl) await extractAndApplyColors(newUrl)
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
      {/* 表面主题应用器：全站写入 html[data-surface]，渲染 null */}
      <SurfaceThemeApplier />

      {/* 全局控制面板
          relative z-9999：把 GCP 整棵子树抬到 host chrome 顶层 stacking context，
          避免 main(z-10) 内全屏 TApp / fixed iframe 在移动端合成层上盖住面板 */}
      <div id="global-control-panel-root" className="relative z-9999">
        <GlobalControlPanel />
      </div>

      {/* 背景 */}
      <div id="bg-container" className="fixed inset-0 -z-10 overflow-hidden">
        <div
          id="wallpaper"
          className="absolute inset-0 bg-cover bg-center bg-no-repeat transition-opacity duration-700 ease-in-out"
        ></div>
        <div
          id="bg-gradient"
          className="absolute inset-0 bg-linear-to-b from-transparent from-35% via-white/40 via-55% to-white/90 to-85% transition-opacity duration-500 ease-out"
        ></div>
        {anim.level !== 'none' && (
          <div className="absolute inset-0 bg-grid-pattern opacity-[0.02]"></div>
        )}
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
      <div
        className="fixed z-100 pointer-events-none
        bottom-6 left-6
        md:bottom-6 md:left-30
        flex flex-col gap-3 max-w-xs"
      >
        {/* 后端未连接提示 - 只在曾经连接过但现在断开时显示 */}
        {backendConnected === false && hasEverConnected && (
          <div className="pointer-events-auto animate-fade-in">
            <div className="glass rounded-xl px-4 py-3 shadow-lg border border-red-200/50 dark:border-red-800/50 bg-red-50/80 dark:bg-red-950/80 backdrop-blur-md">
              <div className="flex items-center gap-3">
                <div className="shrink-0">
                  <div className="w-2 h-2 bg-red-500 rounded-full animate-pulse"></div>
                </div>
                <div>
                  <p className="text-sm font-medium text-red-900 dark:text-red-100">
                    {t.setup.backendDisconnected}
                  </p>
                  <p className="text-xs text-red-700 dark:text-red-300 mt-0.5">
                    {t.setup.reconnecting}
                  </p>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 全局 Toast 通知 */}
      <ToastContainer />

      {/* 主内容区域 */}
      <main className="relative z-10">{children}</main>

      {/* 全局设置弹窗 - 懒加载，整个应用只渲染一次 */}
      <Suspense fallback={null}>
        <SocialNetworkSettingsModal />
        <ReportCardSettingsModal />
        <GamePresenceSettingsModal />
      </Suspense>

      {/* 站点底部信息 */}
      <SiteFooter isHomePage={location.pathname === '/'} />
    </>
  )
}

export default AppLayout
