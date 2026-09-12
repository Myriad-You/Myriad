import type { ReactNode } from 'react'
import type {
  WidgetConfig,
  WidgetGridHandle,
  WidgetSize,
  WidgetType,
} from '../components/widgetGridTypes'
import type { HomeDashboardLayouts, HomeLayoutMode } from '../utils/homeLayout'
import type { HomeLayoutAssetMap } from '../utils/homeLayoutTransfer'

import type { StickerCrop } from '../utils/homeStickerCrop'
import { FaCog, FaCompress, FaEdit, FaExpand, LuSparkles } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import { Avatar } from '../components/Avatar'
import { HomeLayoutTransferButtons } from '../components/home/HomeLayoutTransfer'
import { HomeStickerDialog } from '../components/home/HomeStickerDialog'
import { TitleFontSelector } from '../components/TitleFontSelector'
import {
  getTourSnapshot,
  stopTour,
  subscribeTour,
} from '../components/tour/tourEngine'
import {
  homeEditTourDockPose,
  setHomeEditSurface,
} from '../components/tour/tourLogic'
import WidgetGrid, { startGridLibraryDrag } from '../components/WidgetGrid'
import WidgetLibraryIsland from '../components/WidgetLibraryIsland'
import {
  getBuiltinWidgets,
  preloadBuiltinWidgets,
} from '../components/widgets/builtinWidgets'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useImmersiveChrome } from '../contexts/NavigationContext'
import { useHomeScheduler, usePageReady } from '../hooks/animation'
import { useEditModeEscape } from '../hooks/useEditModeEscape'
import { usePageSeo } from '../hooks/usePageSeo'
import {
  useBreakpoints,
  useDesktopLayoutBand,
} from '../hooks/useSharedEventListener'
import { useSiteOwnerProfile } from '../hooks/useSiteOwnerProfile'
import { useTappWidgets } from '../hooks/useTappWidgets'
import { useResolvedTitleColor, useTitleFont } from '../hooks/useTitleFont'
import { ensureMotionReady } from '../lib/lazyMotion'
import {
  cloneHomeWidgets,
  createHomeStickerItem,
  effectiveHomeLayoutMode,
  homeLayoutsHaveTiles,
  isHomeStickerItem,
  isHomeWidgetItem,
  layoutsAfterWidgetRegistry,
  layoutsForFirstPaint,
  parseDashboardLayoutJson,
  parseHomeLayoutMode,
  peekStoredHomeLayoutMode,
  persistHomeLayoutMode,
  serializeDashboardLayout,
  shouldAcceptHomeLayoutApply,
  stickerPixelSize,
} from '../utils/homeLayout'
import { restoreStickerAssets } from '../utils/homeLayoutStickerAssets'
import { stickerCropForSlot } from '../utils/homeStickerCrop'
import { generateHomeSticker, uploadHomeSticker } from '../utils/homeStickers'
import { stickerAspectKey } from '../utils/homeStickerSize'
import { buildHomePageSeo } from '../utils/modulePageSeo'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { hasSessionHint } from '../utils/sessionDetection'
import { showError, showSuccess, showWarning } from '../utils/toastManager'
import { userFacingError } from '../utils/userFacingError'
import { widgetSizeSpan } from '../utils/widgetSizeScale'
import '../components/home/HomeStickerDialog.css'
import './Home.css'

function readHomeEditTourDockPose() {
  const snapshot = getTourSnapshot()
  return homeEditTourDockPose(snapshot.tourId, snapshot.step?.id ?? null)
}

function HomeStatusBarSlot({
  open,
  side,
  children,
}: {
  open: boolean
  side: 'before' | 'after'
  children: ReactNode
}) {
  return (
    <div
      className={`home-status-bar__slot${open ? ' is-open' : ''}`}
      data-side={side}
      inert={!open ? true : undefined}
      aria-hidden={!open || undefined}
    >
      <div className="home-status-bar__slot-inner">
        <div className="home-status-bar__tools">{children}</div>
      </div>
    </div>
  )
}

export default function Home() {
  useHomeScheduler()

  const { isAuthenticated, hasChecked, checkAuth, isAdmin } = useAuth()
  const { t, format } = useI18n()
  const navigate = useNavigate()
  const isPageReady = usePageReady()
  // Same viewportBands as WidgetGrid (phone≤767 / desktop≥1078).
  const { isMobile: isPhoneBand } = useBreakpoints()
  const isDesktopBand = useDesktopLayoutBand()
  const isNotPhoneBand = !isPhoneBand

  // Site title/description; canonical is always /.
  usePageSeo(useMemo(() => buildHomePageSeo(), []))
  const [layouts, setLayouts] = useState<HomeDashboardLayouts>({
    standard: [],
    free: [],
  })
  const layoutsRef = useRef(layouts)
  layoutsRef.current = layouts
  const [layoutMode, setLayoutMode] = useState<HomeLayoutMode | null>(() =>
    typeof window === 'undefined'
      ? null
      : peekStoredHomeLayoutMode(window.localStorage),
  )
  const [isEditMode, setIsEditMode] = useState(false)
  const tourDockPose = useSyncExternalStore(
    subscribeTour,
    readHomeEditTourDockPose,
    readHomeEditTourDockPose,
  )
  const [layoutFade, setLayoutFade] = useState<'out' | 'in' | null>(null)
  const layoutFadeTimersRef = useRef<{ out?: number; in?: number }>({})
  useEffect(() => {
    return () => {
      if (layoutFadeTimersRef.current.out) {
        window.clearTimeout(layoutFadeTimersRef.current.out)
      }
      if (layoutFadeTimersRef.current.in) {
        window.clearTimeout(layoutFadeTimersRef.current.in)
      }
    }
  }, [])
  const [stickerPicking, setStickerPicking] = useState(false)
  const layoutSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const layoutImportInFlightRef = useRef(false)
  const gridRef = useRef<WidgetGridHandle>(null)
  useImmersiveChrome('home-edit-mode', isEditMode)
  useEditModeEscape(isEditMode, () => {
    if (getTourSnapshot().active) stopTour('abort')
    setIsEditMode(false)
  })
  useEffect(() => {
    setHomeEditSurface(isEditMode)
    return () => setHomeEditSurface(false)
  }, [isEditMode])
  const toggleEditMode = useCallback(() => {
    if (getTourSnapshot().active) stopTour('abort')
    setIsEditMode((current) => !current)
  }, [])
  useEffect(() => {
    if (!stickerPicking) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      setStickerPicking(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [stickerPicking])
  const onLibraryDragStart = useCallback(
    (
      event: Parameters<typeof startGridLibraryDrag>[1],
      widgetTypeId: string,
    ) => {
      startGridLibraryDrag(gridRef.current, event, widgetTypeId)
    },
    [],
  )
  // HTTP cache; avatar-changed forces no-store.
  const { profile: userInfo, avatarEpoch } = useSiteOwnerProfile({
    fallbackName: 'Myriad Dashboard',
    fallbackBio: t.home.defaultBio,
  })
  // '' = server value not yet loaded; never flash a hardcoded 'Dashboard'.
  const [dashboardTitle, setDashboardTitle] = useState('')
  const [csrfToken, setCsrfToken] = useState<string>('')

  const { currentFont, titleFontSize } = useTitleFont()
  const titleColorCss = useResolvedTitleColor()

  const DEFAULT_WIDGETS: WidgetConfig[] = useMemo(
    () => [
      {
        id: 'default-welcome',
        type: 'welcome',
        size: '4x2',
        position: { x: 0, y: 0 },
      },
      {
        id: 'default-weather',
        type: 'weather',
        size: '2x2',
        position: { x: 4, y: 0 },
      },
      {
        id: 'default-quote',
        type: 'quote',
        size: '2x2',
        position: { x: 6, y: 0 },
      },
    ],
    [],
  )

  const AVAILABLE_WIDGETS: WidgetType[] = useMemo(
    () => getBuiltinWidgets(t.widgets, 'home'),
    [t.widgets],
  )

  const { tappWidgets, isLoading: isTappWidgetsLoading } = useTappWidgets()

  const ALL_AVAILABLE_WIDGETS = useMemo(() => {
    return [...AVAILABLE_WIDGETS, ...tappWidgets]
  }, [AVAILABLE_WIDGETS, tappWidgets])

  const resolvedLayoutMode: HomeLayoutMode = layoutMode ?? 'standard'
  const effectiveMode = effectiveHomeLayoutMode(
    resolvedLayoutMode,
    isDesktopBand,
  )
  const isFreeLayout = effectiveMode === 'free'
  const widgets = useMemo(() => {
    if (resolvedLayoutMode === 'free' && !isDesktopBand) {
      const source =
        layouts.free.length > 0 ? layouts.free : layouts.standard
      return source.filter(isHomeWidgetItem)
    }
    return layouts[effectiveMode]
  }, [resolvedLayoutMode, isDesktopBand, layouts, effectiveMode])
  const heroTitle = dashboardTitle.trim() || userInfo?.name || ''
  const [stickerDraft, setStickerDraft] = useState<{
    size: WidgetSize
    position: { x: number; y: number }
    anchor: {
      top: number
      left: number
      width: number
      height: number
      right: number
      bottom: number
    }
  } | null>(null)
  const [stickerBusy, setStickerBusy] = useState(false)
  const [stickerError, setStickerError] = useState('')
  const showHomeAdminActions = Boolean(isAdmin && isDesktopBand)

  useEffect(() => {
    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }
  }, [hasChecked, checkAuth])

  // Force server CSRF; avoid dual-cache drift after axios rotation.
  useEffect(() => {
    async function fetchCsrfToken() {
      if (isAuthenticated && hasChecked) {
        try {
          const { getCSRFToken } = await import('../utils/csrf')
          const { invalidateCsrfCache } = await import('../utils/userInfoCache')
          invalidateCsrfCache()
          const token = await getCSRFToken(true)
          if (token) {
            setCsrfToken(token)
          }
        } catch {
          // CSRF fetch is best-effort.
        }
      }
    }
    fetchCsrfToken()
  }, [isAuthenticated, hasChecked])

  const [rawLayouts, setRawLayouts] = useState<HomeDashboardLayouts | null>(
    null,
  )
  // Invalidate a late first-paint setLayouts once the Tapp registry is ready.
  const layoutApplyGenerationRef = useRef(0)

  // Motion loads with config; grid entry needs real motion, not shim frames.
  useEffect(() => {
    void ensureMotionReady()
  }, [])

  useEffect(() => {
    // Preload lazy widgets + report-card pack before mount (3s cap). report-* must not split during render.
    const fallbackLayouts = (): HomeDashboardLayouts => ({
      standard: DEFAULT_WIDGETS,
      free: cloneHomeWidgets(DEFAULT_WIDGETS),
    })

    async function applyLayouts(next: HomeDashboardLayouts) {
      const generation = ++layoutApplyGenerationRef.current
      await Promise.race([
        Promise.all([
          preloadBuiltinWidgets(
            [...next.standard, ...next.free].map((widget) => widget.type),
          ),
          ensureMotionReady(),
        ]),
        new Promise((resolve) => setTimeout(resolve, 3000)),
      ])
      if (
        !shouldAcceptHomeLayoutApply(
          generation,
          layoutApplyGenerationRef.current,
        )
      ) {
        return
      }
      setLayouts(next)
    }

    async function loadDashboardConfig() {
      try {
        const data = await getUIConfigDeduped()
        const mode = parseHomeLayoutMode(data.dashboard_layout_mode)
        persistHomeLayoutMode(
          mode,
          typeof window === 'undefined' ? null : window.localStorage,
        )
        setLayoutMode(mode)
        setDashboardTitle(data.dashboard_title || 'Dashboard')

        if (data.dashboard_layout) {
          try {
            const parsed = parseDashboardLayoutJson(data.dashboard_layout)
            setRawLayouts(parsed)
            // First paint: do not filter by registry; unknown types already have placeholders.
            await applyLayouts(
              homeLayoutsHaveTiles(parsed)
                ? layoutsForFirstPaint(parsed)
                : fallbackLayouts(),
            )
          } catch (e) {
            console.error('解析仪表盘布局失败:', e)
            await applyLayouts(fallbackLayouts())
          }
        } else {
          await applyLayouts(fallbackLayouts())
        }
      } catch (err) {
        console.error('加载配置失败:', err)
        persistHomeLayoutMode(
          'standard',
          typeof window === 'undefined' ? null : window.localStorage,
        )
        setLayoutMode('standard')
        await applyLayouts(fallbackLayouts())
        setDashboardTitle('Dashboard')
      }
    }
    loadDashboardConfig()
  }, [])

  useEffect(() => {
    if (isTappWidgetsLoading || !rawLayouts || tappWidgets.length === 0) return

    const next = layoutsAfterWidgetRegistry(
      rawLayouts,
      new Set(ALL_AVAILABLE_WIDGETS.map((w) => w.id)),
    )

    if (!homeLayoutsHaveTiles(next)) return

    const sameSide = (a: WidgetConfig[], b: WidgetConfig[]) =>
      a.length === b.length &&
      a.every(
        (widget, i) => widget.id === b[i].id && widget.type === b[i].type,
      )
    const prev = layoutsRef.current
    if (
      sameSide(prev.standard, next.standard) &&
      sameSide(prev.free, next.free)
    ) {
      return
    }

    layoutApplyGenerationRef.current += 1
    setLayouts({
      standard: next.standard.length > 0 ? next.standard : prev.standard,
      free: next.free,
    })
  }, [isTappWidgetsLoading, tappWidgets, rawLayouts, ALL_AVAILABLE_WIDGETS])

  const commitLayoutMode = (next: HomeLayoutMode) => {
    persistHomeLayoutMode(
      next,
      typeof window === 'undefined' ? null : window.localStorage,
    )
    setLayoutMode(next)
    if (!isAdmin) return
    void (async () => {
      try {
        const { getCSRFToken } = await import('../utils/csrf')
        const token = (await getCSRFToken(true)) || csrfToken
        if (!token) {
          showError(t.errors.csrfUnavailable)
          return
        }
        if (token !== csrfToken) setCsrfToken(token)
        const res = await fetch(`${API_URL}/api/config/dashboard`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': token,
          },
          credentials: 'include',
          body: JSON.stringify({ layout_mode: next }),
        })
        if (!res.ok) {
          throw new Error(`Failed to save dashboard layout mode: HTTP ${res.status}`)
        }
      } catch (err) {
        console.error('保存首页布局模式失败:', err)
        showError(userFacingError(err, t.errors.dashboardLayoutSaveFailed))
      }
    })()
  }

  const applyImportedHomeLayout = useCallback(
    async (payload: {
      layouts: HomeDashboardLayouts
      mode: HomeLayoutMode | null
      assets?: HomeLayoutAssetMap
    }) => {
      if (!isAdmin) return
      layoutImportInFlightRef.current = true
      if (layoutSaveTimerRef.current) {
        clearTimeout(layoutSaveTimerRef.current)
        layoutSaveTimerRef.current = null
      }
      try {
        const { getCSRFToken } = await import('../utils/csrf')
        const { clearDedupCache } = await import('../utils/requestDedup')
        const token = (await getCSRFToken(true)) || csrfToken
        if (!token) {
          showError(t.errors.csrfUnavailable)
          return
        }
        if (token !== csrfToken) setCsrfToken(token)
        const restored = await restoreStickerAssets(
          payload.layouts,
          payload.assets ?? {},
          async (image) => {
            const uploaded = await uploadHomeSticker({
              image,
              csrfToken: token,
            })
            return uploaded.imageUrl
          },
        )
        const next = restored.layouts
        const res = await fetch(`${API_URL}/api/config/dashboard`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': token,
          },
          credentials: 'include',
          body: JSON.stringify({
            layout: serializeDashboardLayout(next),
            ...(payload.mode ? { layout_mode: payload.mode } : {}),
          }),
        })
        if (!res.ok) {
          throw new Error(
            `Failed to import dashboard layout: HTTP ${res.status}`,
          )
        }
        if (layoutSaveTimerRef.current) {
          clearTimeout(layoutSaveTimerRef.current)
          layoutSaveTimerRef.current = null
        }
        layoutApplyGenerationRef.current += 1
        setRawLayouts(next)
        setLayouts(next)
        if (payload.mode) {
          persistHomeLayoutMode(
            payload.mode,
            typeof window === 'undefined' ? null : window.localStorage,
          )
          setLayoutMode(payload.mode)
        }
        void preloadBuiltinWidgets(
          [...next.standard, ...next.free].map((widget) => widget.type),
        )
        clearDedupCache(`${API_URL}/api/config/ui`)
        if (restored.failed.length > 0) {
          showWarning(
            format(t.home.importLayoutPartial, {
              count: restored.failed.length,
            }),
          )
        } else {
          showSuccess(t.home.importLayoutSuccess)
        }
      } catch (err) {
        console.error('导入首页布局失败:', err)
        showError(userFacingError(err, t.home.importLayoutFailed))
      } finally {
        layoutImportInFlightRef.current = false
      }
    },
    [csrfToken, isAdmin, t, format],
  )

  const handleLayoutModeToggle = () => {
    if (layoutFade) return
    const next: HomeLayoutMode =
      resolvedLayoutMode === 'free' ? 'standard' : 'free'
    const reduce =
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (reduce) {
      commitLayoutMode(next)
      return
    }
    if (layoutFadeTimersRef.current.out) {
      window.clearTimeout(layoutFadeTimersRef.current.out)
    }
    if (layoutFadeTimersRef.current.in) {
      window.clearTimeout(layoutFadeTimersRef.current.in)
    }
    setLayoutFade('out')
    layoutFadeTimersRef.current.out = window.setTimeout(() => {
      commitLayoutMode(next)
      requestAnimationFrame(() => {
        requestAnimationFrame(() => setLayoutFade('in'))
      })
      layoutFadeTimersRef.current.in = window.setTimeout(() => {
        setLayoutFade(null)
        layoutFadeTimersRef.current.in = undefined
      }, 240)
      layoutFadeTimersRef.current.out = undefined
    }, 180)
  }

  // Debounce 500ms (same as control panel); UI updates immediately.
  const handleWidgetsChange = (newWidgets: WidgetConfig[]) => {
    if (layoutImportInFlightRef.current) return
    const registeredWidgetIds = new Set(ALL_AVAILABLE_WIDGETS.map((w) => w.id))
    let validWidgets = isTappWidgetsLoading
      ? newWidgets
      : newWidgets.filter(
          (w) => isHomeStickerItem(w) || registeredWidgetIds.has(w.type),
        )
    if (effectiveMode === 'standard') {
      validWidgets = validWidgets.filter((w) => !isHomeStickerItem(w))
    }

    const nextLayouts: HomeDashboardLayouts = {
      ...layoutsRef.current,
      [effectiveMode]: validWidgets,
    }
    setLayouts(nextLayouts)

    if (!isAdmin || isTappWidgetsLoading) return

    if (layoutSaveTimerRef.current) {
      clearTimeout(layoutSaveTimerRef.current)
    }
    layoutSaveTimerRef.current = setTimeout(() => {
      void (async () => {
        try {
          const { getCSRFToken } = await import('../utils/csrf')
          const token = (await getCSRFToken(true)) || csrfToken
          if (!token) {
            showError(t.errors.csrfUnavailable)
            return
          }
          if (token !== csrfToken) setCsrfToken(token)
          const res = await fetch(`${API_URL}/api/config/dashboard`, {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-CSRF-Token': token,
            },
            credentials: 'include',
            body: JSON.stringify({
              layout: serializeDashboardLayout(nextLayouts),
            }),
          })
          if (!res.ok) {
            throw new Error(
              `Failed to save dashboard layout: HTTP ${res.status}`,
            )
          }
        } catch (err) {
          console.error('保存小组件配置失败:', err)
          showError(userFacingError(err, t.errors.dashboardLayoutSaveFailed))
        }
      })()
    }, 500)
  }

  const startStickerPick = () => {
    setStickerError('')
    setStickerDraft(null)
    setStickerPicking((on) => !on)
  }

  const handleGenerateSticker = async (
    prompt: string,
    referenceImages: string[] = [],
  ) => {
    if (!stickerDraft || stickerBusy) return
    setStickerBusy(true)
    setStickerError('')
    try {
      const { getCSRFToken } = await import('../utils/csrf')
      const token = (await getCSRFToken(true)) || csrfToken
      if (!token) {
        showError(t.errors.csrfUnavailable)
        return
      }
      const pixels = stickerPixelSize(stickerDraft.size)
      const slot = widgetSizeSpan(stickerDraft.size)
      const generated = await generateHomeSticker({
        prompt,
        width: pixels.width,
        height: pixels.height,
        csrfToken: token,
        referenceImages,
        aspect: stickerAspectKey(stickerDraft.size),
        slotCols: slot.w,
        slotRows: slot.h,
      })
      handleWidgetsChange([
        ...layoutsRef.current.free,
        createHomeStickerItem({
          size: stickerDraft.size,
          position: stickerDraft.position,
          imageUrl: generated.imageUrl,
          prompt,
          crop: stickerCropForSlot(
            generated.width || pixels.width,
            generated.height || pixels.height,
            stickerDraft.size,
          ),
        }),
      ])
      setStickerDraft(null)
    } catch (err) {
      setStickerError(userFacingError(err, t.home.stickerFailed))
    } finally {
      setStickerBusy(false)
    }
  }

  const handleUploadSticker = async (image: string, crop: StickerCrop) => {
    if (!stickerDraft || stickerBusy) return
    setStickerBusy(true)
    setStickerError('')
    try {
      const { getCSRFToken } = await import('../utils/csrf')
      const token = (await getCSRFToken(true)) || csrfToken
      if (!token) {
        showError(t.errors.csrfUnavailable)
        return
      }
      const uploaded = await uploadHomeSticker({
        image,
        csrfToken: token,
      })
      handleWidgetsChange([
        ...layoutsRef.current.free,
        createHomeStickerItem({
          size: stickerDraft.size,
          position: stickerDraft.position,
          imageUrl: uploaded.imageUrl,
          prompt: '',
          crop,
        }),
      ])
      setStickerDraft(null)
    } catch (err) {
      setStickerError(userFacingError(err, t.home.stickerUploadFailed))
    } finally {
      setStickerBusy(false)
    }
  }

  const handleTitleChange = async (newTitle: string) => {
    setDashboardTitle(newTitle)

    if (!isAdmin) return

    try {
      const { getCSRFToken } = await import('../utils/csrf')
      const token = (await getCSRFToken(true)) || csrfToken
      if (!token) {
        showError(t.errors.csrfUnavailable)
        return
      }
      if (token !== csrfToken) setCsrfToken(token)
      const res = await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': token,
        },
        credentials: 'include',
        body: JSON.stringify({
          title: newTitle,
        }),
      })
      if (!res.ok) {
        throw new Error(`Failed to save dashboard title: HTTP ${res.status}`)
      }
    } catch (err) {
      console.error('保存标题失败:', err)
      showError(userFacingError(err, t.errors.dashboardTitleSaveFailed))
    }
  }

  // Widget persists itself; fallback only if the event was not persisted.
  useEffect(() => {
    const handleCustomPlatformsUpdate = async (event: Event) => {
      const customEvent = event as CustomEvent<{
        platforms: unknown[]
        persisted?: boolean
      }>
      if (customEvent.detail?.persisted) return
      if (!isAdmin) return

      try {
        const { getCSRFToken } = await import('../utils/csrf')
        const { clearDedupCache } = await import('../utils/requestDedup')
        const token = (await getCSRFToken(true)) || csrfToken
        if (!token) {
          showError(t.errors.csrfUnavailable)
          return
        }
        const response = await fetch(`${API_URL}/api/config/dashboard`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': token,
          },
          credentials: 'include',
          body: JSON.stringify({
            custom_platforms: JSON.stringify(customEvent.detail.platforms),
          }),
        })
        if (!response.ok) {
          throw new Error(
            `Failed to save custom platforms: HTTP ${response.status}`,
          )
        }
        clearDedupCache(`${API_URL}/api/config/ui`)
      } catch (err) {
        console.error('保存自定义平台失败:', err)
        showError(userFacingError(err, t.errors.customPlatformsSaveFailed))
      }
    }

    window.addEventListener(
      'custom-platforms-update',
      handleCustomPlatformsUpdate,
    )
    return () => {
      window.removeEventListener(
        'custom-platforms-update',
        handleCustomPlatformsUpdate,
      )
    }
  }, [isAdmin, csrfToken, t])

  return (
    <AnimatedView
      className={`home-shell min-h-screen ${
        isDesktopBand
          ? isFreeLayout
            ? 'h-screen overflow-x-hidden overflow-y-auto'
            : 'h-screen overflow-hidden'
          : ''
      }`}
      data-tour="home-agent"
      data-home-band={
        isDesktopBand ? 'desktop' : isPhoneBand ? 'phone' : 'tablet'
      }
      data-home-layout={layoutMode == null ? undefined : effectiveMode}
      data-layout-fade={layoutFade ?? undefined}
    >
      <div className="home-shell__inner h-full flex flex-col">
        <div className="home-shell__stage flex-1 mx-auto w-full flex flex-col gap-4 relative min-h-0">
          <WidgetLibraryIsland
            visible={isEditMode && isDesktopBand}
            availableWidgets={ALL_AVAILABLE_WIDGETS}
            layoutMode={effectiveMode}
            onNewWidgetDragStart={onLibraryDragStart}
            pausePointer={stickerPicking || Boolean(stickerDraft)}
            tourDockPose={tourDockPose}
          />
          <WidgetGrid
            ref={gridRef}
            widgets={widgets}
            availableWidgets={ALL_AVAILABLE_WIDGETS}
            onWidgetsChange={handleWidgetsChange}
            isEditMode={isEditMode}
            layoutMode={effectiveMode}
            tourAnchor="home-grid"
            tourFit={isFreeLayout ? undefined : '.widget-grid-item'}
            stickerPickActive={stickerPicking}
            stickerHighlight={
              stickerDraft
                ? {
                    x: stickerDraft.position.x,
                    y: stickerDraft.position.y,
                    size: stickerDraft.size,
                  }
                : null
            }
            onPickStickerSlot={(slot) => {
              setStickerPicking(false)
              setStickerError('')
              setStickerDraft({
                size: slot.size,
                position: { x: slot.x, y: slot.y },
                anchor: slot.anchor,
              })
            }}
          >
            <h1 className="sr-only">{heroTitle}</h1>
            {isFreeLayout ? null : (
              <div className="relative h-15 shrink-0 z-10 mb-2 p-1">
                {isEditMode ? (
                  <input
                    type="text"
                    aria-label="Dashboard Title"
                    value={dashboardTitle}
                    onChange={(e) => handleTitleChange(e.target.value)}
                    className={`absolute left-1 whitespace-nowrap z-0 bg-transparent border-none outline-none p-0 m-0 w-full ${
                      isNotPhoneBand ? 'block' : 'hidden'
                    }`}
                    style={{
                      top: `calc(30px - ${7.5 * titleFontSize}rem)`,
                      fontSize: `${6 * titleFontSize}rem`,
                      color: titleColorCss,
                      WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                      lineHeight: 1,
                      fontFamily: currentFont.family,
                      fontWeight: 700,
                    }}
                  />
                ) : (
                  <div
                    className="absolute left-1 whitespace-nowrap pointer-events-none z-0 transition-opacity duration-300"
                    style={{
                      top: `calc(30px - ${7.5 * titleFontSize}rem)`,
                      fontSize: `${6 * titleFontSize}rem`,
                      color: titleColorCss,
                      WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                      fontFamily: currentFont.family,
                      fontWeight: 700,
                      opacity: heroTitle ? 1 : 0,
                    }}
                  >
                    {heroTitle}
                  </div>
                )}

                <motion.div
                  className="h-full flex items-center justify-between"
                  initial={{ opacity: 0, x: -20 }}
                  animate={
                    isPageReady ? { opacity: 1, x: 0 } : { opacity: 0, x: -20 }
                  }
                  transition={{
                    duration: 0.3,
                    ease: 'easeOut',
                    delay: isPageReady ? 0.1 : 0,
                  }}
                >
                  <div className="home-status-bar glass shadow-sm">
                    <div className="home-status-bar__row">
                      {userInfo ? (
                        <>
                          <div className="w-8 h-8 rounded-full overflow-hidden border border-gray-200 dark:border-white/10">
                            <Avatar
                              // Remount on avatarEpoch so <img> disk cache cannot keep the old proxy URL.
                              key={`${userInfo.avatar ?? ''}:${avatarEpoch}`}
                              src={userInfo.avatar}
                              name={userInfo.name}
                              className="w-full h-full object-cover"
                            />
                          </div>
                          <div className="flex flex-col justify-center">
                            <div className="text-sm font-bold text-gray-800 dark:text-gray-200 leading-tight">
                              {userInfo.name}
                            </div>
                            {userInfo.bio && (
                              <div className="text-[10px] text-gray-500 dark:text-gray-400 max-w-50 truncate leading-tight">
                                {userInfo.bio}
                              </div>
                            )}
                          </div>
                        </>
                      ) : (
                        <div className="flex items-center gap-2">
                          <div className="w-8 h-8 rounded-full bg-gray-200 dark:bg-white/5 animate-pulse" />
                          <div className="flex flex-col gap-1">
                            <div className="w-20 h-3 bg-gray-200 dark:bg-white/5 rounded animate-pulse" />
                            <div className="w-32 h-2 bg-gray-200 dark:bg-white/5 rounded animate-pulse" />
                          </div>
                        </div>
                      )}

                      {/* Admin + desktop band (≥1078, same threshold as 16-col grid). */}
                      {showHomeAdminActions && (
                        <div className="home-status-bar__actions">
                          <div className="home-status-bar__sep" />

                          <HomeStatusBarSlot open={isEditMode} side="before">
                            <TitleFontSelector csrfToken={csrfToken} />
                          </HomeStatusBarSlot>

                          <button
                            type="button"
                            data-tour="home-edit"
                            onClick={toggleEditMode}
                            className={`flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all ${
                              isEditMode
                                ? 'text-white shadow-md hover:opacity-90'
                                : 'bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10'
                            }`}
                            style={{
                              backgroundColor: isEditMode
                                ? 'var(--color-primary)'
                                : undefined,
                              color: isEditMode
                                ? '#fff'
                                : 'var(--color-primary)',
                            }}
                            aria-label={
                              isEditMode ? t.common.done : t.common.edit
                            }
                          >
                            <FaEdit size={12} aria-hidden />
                            <span className="home-status-bar__mode" aria-hidden>
                              <span data-on={!isEditMode || undefined}>
                                {t.common.edit}
                              </span>
                              <span data-on={isEditMode || undefined}>
                                {t.common.done}
                              </span>
                            </span>
                          </button>

                          <HomeStatusBarSlot open={isEditMode} side="after">
                            <button
                              type="button"
                              data-tour="home-free-layout"
                              onClick={handleLayoutModeToggle}
                              className="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                              style={{ color: 'var(--color-primary)' }}
                              aria-pressed={false}
                              aria-label={t.home.switchToFreeLayout}
                              title={t.home.switchToFreeLayout}
                            >
                              <FaExpand size={12} />
                              {t.home.freeLayout}
                            </button>
                            <HomeLayoutTransferButtons
                              buttonClassName="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10 disabled:opacity-50"
                              buttonStyle={{ color: 'var(--color-primary)' }}
                              layouts={layouts}
                              mode={resolvedLayoutMode}
                              disabled={layoutFade !== null}
                              onImport={applyImportedHomeLayout}
                            />
                          </HomeStatusBarSlot>

                          <button
                            type="button"
                            onClick={() => navigate('/config')}
                            className="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                            style={{ color: 'var(--color-primary)' }}
                            title={t.nav.config}
                            aria-label={t.nav.config}
                          >
                            <FaCog size={12} />
                            {t.nav.config}
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                </motion.div>
              </div>
            )}
          </WidgetGrid>
        </div>
      </div>
      {stickerPicking ? (
        <div className="home-sticker-pick-hint">{t.home.stickerPickHint}</div>
      ) : null}
      {isDesktopBand && isFreeLayout && showHomeAdminActions ? (
        <div className="home-layout-rail" data-library-dock-chrome="">
          <motion.div
            className="home-layout-rail__island"
            initial={{ opacity: 0, y: 12, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            transition={{ type: 'spring', damping: 36, stiffness: 240 }}
          >
            {isEditMode ? (
              <div className="home-layout-rail__cluster">
                <button
                  type="button"
                  data-tour="home-free-layout"
                  className={`home-layout-rail__btn ${
                    isFreeLayout ? 'is-active' : ''
                  }`}
                  onClick={handleLayoutModeToggle}
                  aria-pressed={isFreeLayout}
                  aria-label={
                    isFreeLayout
                      ? t.home.switchToStandardLayout
                      : t.home.switchToFreeLayout
                  }
                  title={
                    isFreeLayout
                      ? t.home.switchToStandardLayout
                      : t.home.switchToFreeLayout
                  }
                >
                  {isFreeLayout ? (
                    <FaCompress size={13} />
                  ) : (
                    <FaExpand size={13} />
                  )}
                  {isFreeLayout ? t.home.standardLayout : t.home.freeLayout}
                </button>
                {isFreeLayout && showHomeAdminActions ? (
                  <button
                    type="button"
                    data-tour="home-sticker"
                    className={`home-layout-rail__btn ${
                      stickerPicking ? 'is-active' : ''
                    }`}
                    onClick={startStickerPick}
                    aria-label={t.home.createSticker}
                    aria-pressed={stickerPicking}
                  >
                    <LuSparkles size={13} />
                    {t.home.createSticker}
                  </button>
                ) : null}
                <HomeLayoutTransferButtons
                  buttonClassName="home-layout-rail__btn"
                  layouts={layouts}
                  mode={resolvedLayoutMode}
                  disabled={layoutFade !== null}
                  onImport={applyImportedHomeLayout}
                />
              </div>
            ) : null}
            {isEditMode && isFreeLayout && showHomeAdminActions ? (
              <div className="home-layout-rail__rule" />
            ) : null}
            {isFreeLayout && showHomeAdminActions ? (
              <div className="home-layout-rail__cluster">
                {isEditMode ? (
                  <TitleFontSelector
                    csrfToken={csrfToken}
                    buttonClassName="home-layout-rail__btn"
                    showHeroOptions={false}
                  />
                ) : null}
                <button
                  type="button"
                  data-tour="home-edit"
                  className={`home-layout-rail__btn ${
                    isEditMode ? 'is-active' : ''
                  }`}
                  onClick={toggleEditMode}
                  aria-pressed={isEditMode}
                  aria-label={isEditMode ? t.common.done : t.common.edit}
                  title={isEditMode ? t.common.done : t.common.edit}
                >
                  <FaEdit size={13} />
                  {isEditMode ? t.common.done : t.common.edit}
                </button>
                <button
                  type="button"
                  className="home-layout-rail__btn"
                  onClick={() => navigate('/config')}
                  aria-label={t.nav.config}
                  title={t.nav.config}
                >
                  <FaCog size={13} />
                  {t.nav.config}
                </button>
              </div>
            ) : null}
          </motion.div>
        </div>
      ) : null}
      {stickerDraft ? (
        <HomeStickerDialog
          size={stickerDraft.size}
          busy={stickerBusy}
          error={stickerError}
          anchor={stickerDraft.anchor}
          onCancel={() => {
            if (!stickerBusy) {
              setStickerDraft(null)
              setStickerError('')
            }
          }}
          onGenerate={handleGenerateSticker}
          onUpload={handleUploadSticker}
        />
      ) : null}
    </AnimatedView>
  )
}
