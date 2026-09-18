import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { useLocation } from 'react-router-dom'
import GlobalControlPanel from '../components/GlobalControlPanel'
import NavigationIsland from '../components/NavigationIsland'
import { SiteFooter } from '../components/SiteFooter'
import { SurfaceThemeApplier } from '../components/SurfaceThemeApplier'
import { ToastContainer } from '../components/ToastContainer'
import { TourHint, TourOverlay } from '../components/tour'


import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
// Live <html data-nav-layout> after first paint is owned by NavigationIsland
// crossfade (chromeLayout). AppLayout only seeds FOUC once.
import { useIdleEffect, useVisibilityInterval } from '../hooks/animation'
import {
  isExlight,
  isReducedAnimation,
  useAnimationLevel,
} from '../hooks/useAnimationLevel'
import { useAuthUrlFeedback } from '../hooks/useAuthUrlFeedback'
import { useEvocativeWallpaper } from '../hooks/useEvocativeWallpaper'
import { useNavAutoHide } from '../hooks/useNavAutoHide'
import { usePageViewTracker } from '../hooks/usePageViewTracker'
import { useScrollOptimization } from '../hooks/useScrollOptimization'
import { useSystemSetupCheck } from '../hooks/useSystemSetupCheck'
import {
  invalidateWallpaperLoadCache,
  useWallpaper,
} from '../hooks/useWallpaper'
import {
  isWidgetSettingsHostArmed,
  subscribeWidgetSettingsHost,
} from '../lib/widgetSettingsHost'
import { ensureCfgAccentSync } from '../utils/cfgAccent'
import { applyColorPalette } from '../utils/colorPalette'
import {
  applyNavLayoutToDocument,
  getNavLayoutSnapshot,
} from '../utils/navLayout'
import {
  getColorFromCache,
  saveColorToCache,
  shouldApplyColorExtraction,
} from '../utils/wallpaperColorCache'
import { wallpaperState } from '../utils/wallpaperState'
import './AppLayout.css'

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
const TappShortcutSettingsModal = lazy(() =>
  import('../components/widgets/TappShortcutWidget').then((m) => ({
    default: m.TappShortcutSettingsModal,
  })),
)

function DeferredWidgetSettingsModals() {
  const armed = useSyncExternalStore(
    subscribeWidgetSettingsHost,
    isWidgetSettingsHostArmed,
    isWidgetSettingsHostArmed,
  )
  if (!armed) return null
  return (
    <Suspense fallback={null}>
      <SocialNetworkSettingsModal />
      <ReportCardSettingsModal />
      <GamePresenceSettingsModal />
      <TappShortcutSettingsModal />
    </Suspense>
  )
}

interface AppLayoutProps {
  children: React.ReactNode
}

/** Scroll start/stop state belongs here; the layout only needs its DOM effects. */
function PageScrollEffects() {
  useScrollOptimization({ enabled: true })
  return null
}

export function AppLayout({ children }: AppLayoutProps) {
  const location = useLocation()
  const { t } = useI18n()
  const [backendConnected, setBackendConnected] = useState<boolean | null>(null)
  const [hasEverConnected, setHasEverConnected] = useState(false)
  const anim = useAnimationLevel()
  const [libraryCanvasActive, setLibraryCanvasActive] = useState(false)

  // Seed <html data-nav-layout> once for FOUC; subsequent flips mid-crossfade
  // by NavigationIsland so the island never teleports while still opaque.
  useLayoutEffect(() => {
    applyNavLayoutToDocument(getNavLayoutSnapshot())
  }, [])

  // LibraryGrid marks canvas in its layout effect. React processes this state
  // update before paint, so the evocative RAF stops without a transition flash.
  // Do not wrap it in flushSync: this callback itself runs during React's layout
  // phase, where forcing a nested synchronous flush is unsupported.
  useLayoutEffect(() => {
    const syncLibraryCanvasMode = () => {
      const next =
        location.pathname === '/library' &&
        document.documentElement.dataset.libraryCanvas === 'active'
      setLibraryCanvasActive(next)
    }

    window.addEventListener(
      'libraryCanvasModeChanged',
      syncLibraryCanvasMode,
    )
    syncLibraryCanvasMode()
    return () => {
      window.removeEventListener(
        'libraryCanvasModeChanged',
        syncLibraryCanvasMode,
      )
    }
  }, [location.pathname])

  // OAuth account-link success/error query → toast + clean URL
  useAuthUrlFeedback()

  usePageViewTracker()

  useSystemSetupCheck()

  // 设置强调色 --cfg-accent：与 Hero adaptive 同源，随主题/壁纸重算
  useEffect(() => {
    ensureCfgAccentSync()
  }, [])

  const { loadWallpaper: loadWallpaperFromHook } = useWallpaper()

  const [evocativeConfig, setEvocativeConfig] = useState({
    parallax: true,
    dynamicBlur: false,
    ripple: false,
    fps: 30,
    rippleQuality: 0.85,
    blur: 3,
  })

  // 仅 exlight 或资料库 canvas 强制关；light 档仍尊重用户开关。
  const evocativeForceOff = isExlight(anim) || libraryCanvasActive
  useEvocativeWallpaper('wallpaper', {
    parallax: {
      enabled: evocativeConfig.parallax && !evocativeForceOff,
      enableGyroscope: true,
      enableMouse: true,
      maxOffset: 8,
      scale: 1.02,
    },
    dynamicBlur: {
      enabled: evocativeConfig.dynamicBlur && !evocativeForceOff,
      baseBlur: evocativeConfig.blur,
      unblurZone: 0.4,
      blurZone: 0.6,
    },
    ripple: {
      enabled: evocativeConfig.ripple && !evocativeForceOff,
    },
    // light 档略降帧率/画质，减轻中档机负担但仍可感知动效
    fps: evocativeForceOff
      ? 30
      : isReducedAnimation(anim)
        ? Math.min(evocativeConfig.fps, 30)
        : evocativeConfig.fps,
    rippleQuality: evocativeForceOff
      ? 0.5
      : isReducedAnimation(anim)
        ? Math.min(evocativeConfig.rippleQuality, 0.7)
        : evocativeConfig.rippleQuality,
  })

  const extractAndApplyColors = useCallback(async (url: string) => {
    if (!wallpaperState.isUrlActive(url)) return

    const cachedColors = getColorFromCache(url)
    if (cachedColors) {
      if (wallpaperState.isUrlActive(url)) applyColorPalette(cachedColors)
      return
    }

    const checkResult = await shouldApplyColorExtraction(url)
    if (!checkResult.shouldApply) return

    try {
      const { extractColorsFromImage } = await import('../utils/colorExtractor')
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

  const loadWallpaper = useCallback(async () => {
    const wallpaperResult = await loadWallpaperFromHook()
    if (!wallpaperResult) return

    // Evocative 配置与壁纸图是否加载成功解耦
    const ev = wallpaperResult.evocative
    setEvocativeConfig({
      parallax: ev?.parallax ?? true,
      dynamicBlur: ev?.dynamicBlur ?? false,
      ripple: ev?.ripple ?? false,
      fps: ev?.fps ?? 30,
      rippleQuality: ev?.rippleQuality ?? 0.85,
      blur: wallpaperResult.blur,
    })

    if (wallpaperResult.actualUrl) {
      await extractAndApplyColors(wallpaperResult.actualUrl)
    }
  }, [loadWallpaperFromHook, extractAndApplyColors])

  const checkBackendRef = useRef<() => Promise<void>>(undefined)
  checkBackendRef.current = async () => {
    try {
      const response = await fetch(`${API_URL}/health`, {
        method: 'GET',
        signal: AbortSignal.timeout(5000),
      })
      setBackendConnected(response.ok)
      if (response.ok) {
        setHasEverConnected(true)
      }
    } catch {
      setBackendConnected(false)
    }
  }

  useIdleEffect(
    () => {
      checkBackendRef.current?.()
    },
    [],
    { priority: 'high' },
  )

  useVisibilityInterval(
    () => {
      checkBackendRef.current?.()
    },
    { delay: 30000, enabled: true },
  )

  // 先挂上呼吸占位，等图片预加载完成后再渐显壁纸（见 useWallpaper.applyWallpaperToDOM）
  const hasInitializedRef = useRef(false)
  useEffect(() => {
    if (hasInitializedRef.current) return
    hasInitializedRef.current = true

    // 首次进入：若壁纸尚未可见，挂上呼吸占位（勿写进 React className）
    const bg = document.getElementById('bg-container')
    const wallpaperEl = document.getElementById('wallpaper')
    if (
      bg &&
      wallpaperEl &&
      !wallpaperEl.classList.contains('wallpaper-visible')
    ) {
      bg.classList.add('wallpaper-awaiting')
    }

    console.debug('[AppLayout] Initializing wallpaper load...')
    ;(async () => {
      try {
        await loadWallpaper()
        console.debug('[AppLayout] Wallpaper load completed')
      } catch (error) {
        console.error('[AppLayout] Wallpaper load failed:', error)
      }
    })()
  }, [])

  useEffect(() => {
    const handleWallpaperChanged = async (e: Event) => {
      const newUrl = (e as CustomEvent).detail?.url
      if (newUrl) await extractAndApplyColors(newUrl)
    }

    window.addEventListener('wallpaperChanged', handleWallpaperChanged)
    return () => {
      window.removeEventListener('wallpaperChanged', handleWallpaperChanged)
    }
  }, [extractAndApplyColors])

  // 配置页保存壁纸 / Evocative 后：清缓存并重新 loadWallpaper（非硬刷）
  useEffect(() => {
    const handleConfigWallpaperReload = () => {
      invalidateWallpaperLoadCache()
      void loadWallpaper()
    }
    window.addEventListener(
      'wallpaperConfigChanged',
      handleConfigWallpaperReload,
    )
    return () => {
      window.removeEventListener(
        'wallpaperConfigChanged',
        handleConfigWallpaperReload,
      )
    }
  }, [loadWallpaper])

  useNavAutoHide()

  return (
    <>
      <PageScrollEffects />
      <SurfaceThemeApplier />

      {/* relative z-9999：GCP 整棵子树抬到 host chrome 顶层 stacking context，
          避免 main(z-10) 内全屏 TApp / fixed iframe 在移动端合成层上盖住面板 */}
      <div id="global-control-panel-root" className="relative z-9999">
        <GlobalControlPanel />
      </div>

      {/* 背景层叠（勿给 #wallpaper 设 z-index，否则会盖住涟漪 canvas 与 #bg-gradient 底部遮罩）：
          呼吸占位 → 壁纸 → 涟漪(JS insert) → 底部渐变遮罩 → 网格
          wallpaper-awaiting 只走 JS classList（此处首挂 + useWallpaper.apply），不要写死在 React className。 */}
      <div
        id="bg-container"
        className="fixed inset-0 -z-10 h-lvh min-h-lvh w-full min-w-full overflow-hidden"
      >
        {/* 首次加载呼吸占位：独立层，z-0，不占 ::before/::after */}
        <div
          id="wallpaper-awaiting-fx"
          className="pointer-events-none absolute inset-0 z-0"
          aria-hidden="true"
        />
        <div
          id="wallpaper"
          className="absolute bg-cover bg-center bg-no-repeat"
        ></div>
        <div
          id="bg-gradient"
          className="pointer-events-none absolute inset-0 z-[2] bg-linear-to-b from-transparent from-35% via-white/40 via-55% to-white/90 to-85% transition-opacity duration-500 ease-out"
        ></div>
        {!isExlight(anim) && (
          <div className="pointer-events-none absolute inset-0 z-[3] bg-grid-pattern opacity-[0.02]"></div>
        )}
      </div>

      <NavigationIsland />

      {/* 父容器 pointer-events-none，子元素需要 pointer-events-auto */}
      <div
        className="fixed z-100 pointer-events-none
        bottom-6 left-6
        md:bottom-6 md:left-30
        flex flex-col gap-3 max-w-xs"
      >
        {backendConnected === false && hasEverConnected && (
          <div className="pointer-events-auto animate-fade-in">
            <div className="glass rounded-xl px-4 py-3 border border-red-200/50 dark:border-red-800/50">
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

      <ToastContainer />
      <TourOverlay />

      <main className="relative z-10">{children}</main>

      <DeferredWidgetSettingsModals />

      <TourHint />
      <SiteFooter isHomePage={location.pathname === '/'} />
    </>
  )
}

export default AppLayout
