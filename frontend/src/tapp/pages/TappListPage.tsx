/**
 * Tapp list page — installed apps + shortcuts to store / playground.
 */

import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from 'react'
import type { ToastType } from '../../components/Toast'
import type { TappAppCardSize } from '../components/TappAppCard'
import type { TappInstance, TappPermission } from '../types'
import {
  FaFolder,
  FaGlobe,
  FaPlus,
  FaTh,
  FaUser,
  MyriadStoreIcon,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
} from '@lib/motionShim'
import {

  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useNavigate } from 'react-router-dom'
import AnimatedView from '../../components/AnimatedView'
import Toast from '../../components/Toast'
import { GlowBackground } from '../../components/widgets/shared/GlowBackground'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { useTappScheduler } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePageSeo } from '../../hooks/usePageSeo'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import { useResolvedTitleColor, useTitleFont } from '../../hooks/useTitleFont'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import { hasSessionHint } from '../../utils/sessionDetection'
import { InstallTappDialog } from '../components/InstallTappDialog'
import { TappPlaygroundIcon } from '../components/PlaygroundIcons'
import {
  applyTappAppCardOrder,
  isSiteOwnerLayoutPending,
  loadTappAppCardLayout,
  loadTappAppCardSizes,
  saveTappAppCardLayout,
  TappAppCard,
  toggleTappAppCardSize,
} from '../components/TappAppCard'
import { TappIcon } from '../components/TappIcon'
import { UninstallConfirmDialog } from '../components/UninstallConfirmDialog'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import { getTappRuntime } from '../runtime'
import { isWebKit } from '../runtime/TappPageSandbox'
import { listTappDetails } from '../services/TappLifecycleApi'
import {
  fetchTappListCardSizes,
  saveTappListCardSizes,
} from '../services/TappListCardSizesApi'
import { resolveManifestText } from '../utils/manifestLocale'
import { buildTappListPageSeo } from '../utils/tappPageSeo'
import {
  TAPP_PLAYGROUND_PATH,
  TAPP_STORE_PATH,
  tappDetailPath,
  tappRunMultiPath,
  tappRunPath,
} from '../utils/tappPaths'
import '../components/TappAppCard.css'

/**
 * Tapp list page component
 */
export function TappListPage() {
  const navigate = useNavigate()
  const { t, locale } = useI18n()
  const { isMobile } = useBreakpoints()
  const { isAdmin, isAuthenticated, hasChecked, checkAuth } = useAuth()
  const animConfig = useAnimationLevel()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.tapp,
    { isAuthenticated: false, isAdmin: false },
  )
  // 🆕 标题字体 Hook
  const { currentFont, titleFontSize } = useTitleFont()
  // 自适应色对齐 Tapp 音乐播放器歌词：对比度推导
  const titleColorCss = useResolvedTitleColor()

  const [tapps, setTapps] = useState<TappInstance[]>([])
  /**
   * Pure site-owner public catalog (no personal-id collision drop).
   * `null` = not loaded yet (do not treat as empty or fall back to runtime filter).
   */
  const [siteTapps, setSiteTapps] = useState<TappInstance[] | null>(null)
  const [runningTapps, setRunningTapps] = useState<Set<string>>(new Set())
  /**
   * Personal list card size (1x1 / 2x1) + order.
   * Logged-in: bound to user row in DB; localStorage is cache.
   * Guests: never seed from personal localStorage (stale owner/user order
   * would flash before remote site layout arrives).
   * Never store site-owner fills here — site scope uses `siteCard*` below.
   */
  const [cardSizes, setCardSizes] = useState<Record<string, TappAppCardSize>>(
    () => (hasSessionHint() ? loadTappAppCardSizes() : {}),
  )
  const [cardOrder, setCardOrder] = useState<string[]>(
    () => (hasSessionHint() ? loadTappAppCardLayout().order : []),
  )
  /** Site-owner public layout (read-only for regular users). */
  const [siteCardSizes, setSiteCardSizes] = useState<
    Record<string, TappAppCardSize>
  >({})
  const [siteCardOrder, setSiteCardOrder] = useState<string[]>([])
  /**
   * Remote list-card-sizes hydrate settled (success or failure).
   * Public list paths wait on this so the first card paint uses final order.
   */
  const [layoutReady, setLayoutReady] = useState(false)
  /**
   * Regular users (non-admin): filter list between personal installs and
   * site-owner public apps. Admins/guests do not use this toggle.
   * Persisted for the tab so refresh keeps the same scope.
   */
  type ListScope = 'mine' | 'site'
  const [listScope, setListScope] = useState<ListScope>(() => {
    if (typeof window === 'undefined') return 'mine'
    try {
      const raw = window.sessionStorage.getItem('tapp.listScope.v1')
      return raw === 'site' ? 'site' : 'mine'
    } catch {
      return 'mine'
    }
  })
  const [dragId, setDragId] = useState<string | null>(null)
  const [dragOverId, setDragOverId] = useState<string | null>(null)
  const dragIdRef = useRef<string | null>(null)
  /** Tear down window-level drag listeners registered for the active session. */
  const dragSessionCleanupRef = useRef<(() => void) | null>(null)
  const suppressOpenRef = useRef(false)
  const [loading, setLoading] = useState(true)
  const [showEmpty, setShowEmpty] = useState(false) // 延迟显示空状态
  // 手动安装 tooltip（锚定安装按钮）
  const [showInstallDialog, setShowInstallDialog] = useState(false)
  const [installAnchor, setInstallAnchor] = useState<HTMLElement | null>(null)
  const [toastMessage, setToastMessage] = useState('')
  const [toastType, setToastType] = useState<ToastType>('info')
  // 卸载确认 tooltip
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const [uninstallTargetId, setUninstallTargetId] = useState<string | null>(
    null,
  )
  const [uninstallTargetName, setUninstallTargetName] = useState('')
  const [uninstallAnchor, setUninstallAnchor] = useState<HTMLElement | null>(
    null,
  )
  const runtime = getTappRuntime()

  usePageSeo(
    useMemo(
      () =>
        buildTappListPageSeo({
          listLabel: t.tapp.listTitle || t.nav.tapp || 'Tapp',
          listDescription: t.tapp.listSubtitle,
          moduleOpenToAll,
        }),
      [t, moduleOpenToAll],
    ),
  )

  const showToastMessage = useCallback(
    (message: string, type: ToastType = 'info') => {
      setToastType(type)
      setToastMessage(message)
    },
    [],
  )

  // 馃幀 鍒濆鍖?Tapp 椤甸潰璋冨害鍣紙缁熶竴鍔ㄧ敾鍗忚皟锛?
  useTappScheduler()

  /** Map site-scope details → list cards; overlay running state from runtime. */
  const mapSiteDetails = useCallback(
    (details: Awaited<ReturnType<typeof listTappDetails>>): TappInstance[] => {
      return details.map((detail) => {
        const existing = runtime.getTapp(detail.id)
        const installationStatus: TappInstance['installationStatus'] =
          detail.status === 'running'
            ? 'running'
            : detail.status === 'error'
              ? 'error'
              : 'installed'
        const isRunning =
          runtime.isRunning(detail.id) || installationStatus === 'running'
        return {
          id: detail.id,
          manifest: detail.manifest,
          status: isRunning
            ? 'running'
            : installationStatus === 'error'
              ? 'error'
              : 'installed',
          installationStatus,
          installedAt: detail.installed_at,
          lastRunAt: detail.last_run_at,
          grantedPermissions: (detail.granted_permissions ||
            []) as TappPermission[],
          // Keep viewer role for actions; mark as site-public for filtering
          userRole: existing?.userRole ?? (isAdmin ? 'admin' : 'user'),
          isTemporary: false,
          isAdminTapp: true,
          visibility: detail.visibility === 'admin' ? 'admin' : 'all',
        }
      })
    },
    [runtime, isAdmin],
  )

  // 加载 Tapp 列表
  const loadTapps = useCallback(
    async (forceSync: boolean = false) => {
      // 如果需要强制同步（如安装后），先从后端刷新
      if (forceSync) {
        await runtime.syncFromBackend(true)
      }

      const allTapps = runtime.getAllTapps()
      setTapps(allTapps)
      // Mine / admin / guest can render immediately; site scope waits on `siteTapps`.
      setLoading(false)

      const running = new Set<string>()
      allTapps.forEach((tapp) => {
        if (runtime.isRunning(tapp.id)) {
          running.add(tapp.id)
        }
      })
      setRunningTapps(running)

      // Site catalog for regular users (complete public list, no personal dedupe).
      // Stay on `null` until this settles so site scope never uses a runtime fallback.
      if (isAuthenticated && !isAdmin) {
        try {
          const details = await listTappDetails('site')
          setSiteTapps(mapSiteDetails(details))
        } catch (error) {
          console.error('Failed to load site Tapp catalog:', error)
          // Keep last successful catalog; first-load failure → empty (not runtime dedupe).
          setSiteTapps((prev) => prev ?? [])
        }
      } else {
        // Admin / guest: site toggle unused; clear pending state.
        setSiteTapps(null)
      }
    },
    [runtime, isAuthenticated, isAdmin, mapSiteDetails],
  )

  /** Non-admin signed-in users can switch personal vs site-owner catalogs. */
  const canToggleListScope = isAuthenticated && !isAdmin

  /** Drop duplicate ids (first wins) — defensive; API also dedupes within a scope. */
  const dedupeById = useCallback((list: TappInstance[]) => {
    const seen = new Set<string>()
    const out: TappInstance[] = []
    for (const item of list) {
      if (!item?.id || seen.has(item.id)) continue
      seen.add(item.id)
      out.push(item)
    }
    return out
  }, [])

  /** Site catalog still in flight — do not empty-flash or use runtime fallback. */
  const siteCatalogPending =
    canToggleListScope && listScope === 'site' && siteTapps === null

  const scopedTapps = useMemo(() => {
    if (!canToggleListScope) return dedupeById(tapps)
    if (listScope === 'site') {
      // null = not loaded yet → empty list while pending (empty CTA gated separately)
      if (siteTapps === null) return []
      return dedupeById(siteTapps)
    }
    // mine: personal / temporary installs only (not site-public)
    return dedupeById(tapps.filter((t) => t.isAdminTapp !== true))
  }, [tapps, siteTapps, listScope, canToggleListScope, dedupeById])

  /** Site scope uses owner layout; mine / admin / guest-primary uses personal state. */
  const useSiteLayout = canToggleListScope && listScope === 'site'
  const activeCardSizes = useSiteLayout ? siteCardSizes : cardSizes
  const activeCardOrder = useSiteLayout ? siteCardOrder : cardOrder

  /**
   * Guest primary + regular-user site scope: hold cards until site layout
   * hydrates so we never paint catalog order then jump to owner order.
   */
  const siteLayoutPending = isSiteOwnerLayoutPending({
    layoutReady,
    isAuthenticated,
    isSiteScope: useSiteLayout,
  })

  const orderedTapps = useMemo(
    () => applyTappAppCardOrder(scopedTapps, activeCardOrder),
    [scopedTapps, activeCardOrder],
  )

  /** Cards ready to paint (apps + public layout when required). */
  const listDisplayPending =
    loading || siteCatalogPending || siteLayoutPending

  /** Layout editing only on personal list (site-owner layout is read-only). */
  const canEditLayout =
    isAuthenticated && (!canToggleListScope || listScope === 'mine')

  // 延迟显示空状态 — 等 loading + site catalog + public layout 都就绪后再判断
  useEffect(() => {
    if (!listDisplayPending && orderedTapps.length === 0) {
      const timer = setTimeout(() => {
        setShowEmpty(true)
      }, 150)
      return () => clearTimeout(timer)
    } else {
      setShowEmpty(false)
    }
  }, [listDisplayPending, orderedTapps.length])

  useEffect(() => {
    let mounted = true

    // 首次访问时检查认证状态
    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }

    // 初始加载：等待同步完成后再获取 Tapp 列表
    const initLoad = async () => {
      await runtime.waitForSync()
      if (mounted) {
        loadTapps()
      }
    }

    initLoad()

    // 鐩戝惉浜嬩欢
    const handleTappChange = () => loadTapps()
    const unsubInstalled = runtime.on('tapp:installed', handleTappChange)
    const unsubUninstalled = runtime.on('tapp:uninstalled', handleTappChange)
    const unsubStarted = runtime.on('tapp:started', handleTappChange)
    const unsubStopped = runtime.on('tapp:stopped', handleTappChange)
    const unsubSyncComplete = runtime.on('sync:complete', handleTappChange)

    return () => {
      mounted = false
      unsubInstalled()
      unsubUninstalled()
      unsubStarted()
      unsubStopped()
      unsubSyncComplete()
    }
  }, [loadTapps, runtime, hasChecked, checkAuth])

  const handleStart = async (tappId: string) => {
    try {
      await runtime.startTapp(tappId)
    } catch (error) {
      console.error('Failed to start Tapp:', error)
    }
  }

  const handleStop = async (tappId: string) => {
    try {
      await runtime.stopTapp(tappId)
    } catch (error) {
      console.error('Failed to stop Tapp:', error)
    }
  }

  /** 安装 / 卸载 tip 互斥：同一时刻只开一个，避免双 portal 叠层与状态打架 */
  const openInstallDialog = useCallback((anchor?: HTMLElement | null) => {
    setShowUninstallDialog(false)
    setUninstallTargetId(null)
    setUninstallTargetName('')
    setUninstallAnchor(null)
    setInstallAnchor(anchor ?? null)
    setShowInstallDialog(true)
  }, [])

  const cancelInstall = useCallback(() => {
    setShowInstallDialog(false)
    setInstallAnchor(null)
  }, [])

  const handleUninstall = useCallback(
    (tappId: string, anchor?: HTMLElement | null) => {
      const tapp = tapps.find((item) => item.id === tappId)
      setShowInstallDialog(false)
      setInstallAnchor(null)
      setUninstallTargetId(tappId)
      setUninstallTargetName(
        tapp ? resolveManifestText(tapp.manifest, locale).name : tappId,
      )
      setUninstallAnchor(anchor ?? null)
      setShowUninstallDialog(true)
    },
    [tapps, locale],
  )

  const handleConfirmUninstall = useCallback(
    async (keepData: boolean) => {
      if (!uninstallTargetId) return
      try {
        await runtime.uninstallTapp(uninstallTargetId, { keepData })
        setShowUninstallDialog(false)
        setUninstallTargetId(null)
        setUninstallAnchor(null)
      } catch (error) {
        console.error('Failed to uninstall Tapp:', error)
        showToastMessage(t.tapp.uninstallFailed || 'Uninstall failed', 'error')
        throw error
      }
    },
    [uninstallTargetId, runtime, showToastMessage, t.tapp.uninstallFailed],
  )

  const cancelUninstall = useCallback(() => {
    setShowUninstallDialog(false)
    setUninstallTargetId(null)
    setUninstallTargetName('')
    setUninstallAnchor(null)
  }, [])

  const handleOpen = useCallback(
    (tappId: string) => {
      if (suppressOpenRef.current) {
        suppressOpenRef.current = false
        return
      }
      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.TAPP_RUN, {
            target: tappId,
            throttleMs: 2000,
          })
        },
      )
      navigate(tappRunPath(tappId))
    },
    [navigate],
  )

  // Hydrate layout: personal prefs stay pure; site-owner layout is separate.
  // Never full-save a display merge (that would sticky-freeze owner sizes).
  // Public paths gate card paint on `layoutReady` (see siteLayoutPending).
  useEffect(() => {
    if (!hasChecked) return
    let cancelled = false
    setLayoutReady(false)

    // Authenticated: seed personal local cache immediately for mine scope.
    // Guests: stay empty until remote site layout (never personal localStorage).
    if (isAuthenticated) {
      const local = loadTappAppCardLayout()
      setCardSizes(local.sizes)
      setCardOrder(local.order)
    } else {
      setCardSizes({})
      setCardOrder([])
    }

    void (async () => {
      try {
        const remote = await fetchTappListCardSizes()
        if (cancelled) return
        setSiteCardSizes(remote.siteSizes)
        setSiteCardOrder(remote.siteOrder)
        if (isAuthenticated) {
          const local = loadTappAppCardLayout()
          // Drop local keys that only mirror site-owner layout (legacy sticky
          // merge pollution). Keep local-only keys that differ from site.
          const localPersonalOnly: Record<string, TappAppCardSize> = {}
          for (const [id, size] of Object.entries(local.sizes)) {
            if (id in remote.sizes) continue
            if (remote.siteSizes[id] === size) continue
            localPersonalOnly[id] = size
          }
          // Pure personal: server wins conflicts; remaining local-only migrate.
          const personalSizes: Record<string, TappAppCardSize> = {
            ...localPersonalOnly,
            ...remote.sizes,
          }
          const personalOrder =
            remote.order.length > 0 ? remote.order : local.order
          setCardSizes(personalSizes)
          setCardOrder(personalOrder)
          saveTappAppCardLayout({
            sizes: personalSizes,
            order: personalOrder,
          })
          const hasLocalOnly = Object.keys(localPersonalOnly).length > 0
          const localOrderOnly =
            remote.order.length === 0 && local.order.length > 0
          if (hasLocalOnly || localOrderOnly) {
            // Migrate pure personal prefs only — never site fills.
            await saveTappListCardSizes({
              sizes: personalSizes,
              order: personalOrder,
            })
          }
        } else {
          // Guest: primary payload is site-owner layout (read-only)
          setCardSizes(remote.sizes)
          setCardOrder(remote.order)
        }
      } catch {
        // Offline / fetch failure: fall back to catalog order once (no flip).
        // Guests: empty order → applyTappAppCardOrder no-ops (catalog).
        // Authed: keep local personal cache already seeded above.
        if (!isAuthenticated) {
          setCardSizes({})
          setCardOrder([])
          setSiteCardSizes({})
          setSiteCardOrder([])
        }
      } finally {
        if (!cancelled) setLayoutReady(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [hasChecked, isAuthenticated])

  /** Persist personal layout only (site layout is owner-controlled). */
  const persistLayout = useCallback(
    (sizes: Record<string, TappAppCardSize>, order: string[]) => {
      saveTappAppCardLayout({ sizes, order })
      void saveTappListCardSizes({ sizes, order }).catch(() => {
        // Network failure: local cache already updated; next load retries
      })
    },
    [],
  )

  const handleToggleCardSize = useCallback(
    (tappId: string) => {
      // Guests / site-owner view: layout is read-only
      if (!canEditLayout) return
      setCardSizes((prev) => {
        const nextSize = toggleTappAppCardSize(prev[tappId] ?? '1x1')
        const next = { ...prev, [tappId]: nextSize }
        persistLayout(next, cardOrder)
        return next
      })
    },
    [canEditLayout, cardOrder, persistLayout],
  )

  const handleToggleListScope = useCallback(() => {
    setListScope((prev) => {
      const next: ListScope = prev === 'mine' ? 'site' : 'mine'
      try {
        window.sessionStorage.setItem('tapp.listScope.v1', next)
      } catch {
        /* ignore */
      }
      return next
    })
  }, [])

  const clearCardDrag = useCallback(() => {
    dragIdRef.current = null
    setDragId(null)
    setDragOverId(null)
    // Remove any window-level session listeners (idempotent)
    const cleanup = dragSessionCleanupRef.current
    if (cleanup) {
      dragSessionCleanupRef.current = null
      cleanup()
    }
  }, [])

  /** Always leave drag UI in a clean state (drop, cancel, escape, leave window). */
  const finishCardDrag = useCallback(() => {
    clearCardDrag()
    // Swallow residual click from mouseup after HTML5 drag; then re-enable open
    suppressOpenRef.current = true
    window.setTimeout(() => {
      suppressOpenRef.current = false
    }, 0)
  }, [clearCardDrag])

  // Unmount safety: never leave drag listeners / ghost state behind
  useEffect(() => {
    return () => {
      dragSessionCleanupRef.current?.()
      dragSessionCleanupRef.current = null
      dragIdRef.current = null
    }
  }, [])

  const handleDragHandleStart = useCallback(
    (e: ReactDragEvent, tappId: string) => {
      if (!canEditLayout) {
        e.preventDefault()
        return
      }
      // Tear down a previous session if start fires without a clean end
      dragSessionCleanupRef.current?.()
      dragSessionCleanupRef.current = null

      suppressOpenRef.current = true
      dragIdRef.current = tappId
      setDragId(tappId)
      setDragOverId(null)
      e.dataTransfer.effectAllowed = 'move'
      e.dataTransfer.setData('text/plain', tappId)

      // Window capture: dragend always fires when a DnD op ends (even if drop
      // target never receives events, or the card unmounts mid-drag).
      const onWindowDragEnd = () => {
        finishCardDrag()
      }
      // Escape / focus loss can cancel without a clean bubble path
      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === 'Escape') finishCardDrag()
      }
      window.addEventListener('dragend', onWindowDragEnd, true)
      window.addEventListener('keydown', onKeyDown, true)
      dragSessionCleanupRef.current = () => {
        window.removeEventListener('dragend', onWindowDragEnd, true)
        window.removeEventListener('keydown', onKeyDown, true)
      }
    },
    [canEditLayout, finishCardDrag],
  )

  const handleDragOverCard = useCallback(
    (e: ReactDragEvent, tappId: string) => {
      const from = dragIdRef.current
      if (from === null || from === tappId) {
        // No active session (or self) — keep highlight clear
        if (from === null) setDragOverId(null)
        return
      }
      e.preventDefault()
      e.dataTransfer.dropEffect = 'move'
      setDragOverId((prev) => (prev === tappId ? prev : tappId))
    },
    [],
  )

  const handleDragLeaveCard = useCallback(
    (e: ReactDragEvent, tappId: string) => {
      const next = e.relatedTarget as Node | null
      if (next && (e.currentTarget as HTMLElement).contains(next)) return
      setDragOverId((prev) => (prev === tappId ? null : prev))
    },
    [],
  )

  const handleDropOnCard = useCallback(
    (e: ReactDragEvent, toId: string) => {
      e.preventDefault()
      e.stopPropagation()
      const fromId = dragIdRef.current
      if (!fromId || fromId === toId || !canEditLayout) {
        finishCardDrag()
        return
      }

      setCardOrder((prev) => {
        // Build order from currently visible apps so drops always resolve
        const base = applyTappAppCardOrder(scopedTapps, prev).map((t) => t.id)
        const fromIndex = base.indexOf(fromId)
        const toIndex = base.indexOf(toId)
        if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return prev
        const next = [...base]
        next.splice(fromIndex, 1)
        next.splice(toIndex, 0, fromId)
        persistLayout(cardSizes, next)
        return next
      })
      // Clear highlight + dragging immediately; dragend will also call finish
      finishCardDrag()
    },
    [canEditLayout, scopedTapps, cardSizes, persistLayout, finishCardDrag],
  )

  const handleDragEndCard = useCallback(() => {
    finishCardDrag()
  }, [finishCardDrag])

  const handleConfigure = (tappId: string) => {
    void import('../../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.TAPP_OPEN_DETAIL, {
          target: tappId,
          throttleMs: 2000,
        })
      },
    )
    navigate(tappDetailPath(tappId))
  }

  return (
    <AnimatedView className="min-h-screen">
      <div className="h-full flex flex-col pt-20 pb-6 px-3 xs:px-4 sm:px-6">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col gap-3 p-2 relative min-h-0">
          {/* 涓婂崐閮ㄥ垎鐣欑櫧锛屼笌棣栭〉 Widget 鍖哄煙瀵归綈 */}
          <div className="hidden lg:block flex-1 min-h-[30vh]" />

          {/* 椤堕儴淇℃伅鏉?- 涓庨椤靛竷灞€涓€鑷? */}
          <div className="relative h-12 shrink-0 z-10">
            {/* 鑳屾櫙鏍囬 */}
            <div
              className="absolute left-0 whitespace-nowrap pointer-events-none z-0 hidden md:block"
              style={{
                top: `calc(24px - ${7.5 * titleFontSize}rem)`,
                fontSize: `${6 * titleFontSize}rem`,
                color: titleColorCss,
                WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                fontFamily: currentFont.family,
                fontWeight: 700,
              }}
            >
              Tapp
            </div>

            <div className="h-full flex items-center justify-between">
              {/* 宸︿晶淇℃伅鍗＄墖 */}
              <div className="h-full glass rounded-xl px-4 py-1 flex items-center gap-3 shadow-sm relative z-10">
                <TappIcon
                  icon={TAPP_ICON_TOKENS.store}
                  name={t.tapp.storeTitle}
                  sizeClass="w-7 h-7"
                />
                <div className="flex flex-col justify-center">
                  <div className="text-sm font-bold text-gray-800 dark:text-gray-200 leading-tight">
                    {canToggleListScope
                      ? listScope === 'site'
                        ? t.tapp.listScopeSite
                        : t.tapp.listScopeMine
                      : t.tapp.listTitle}
                  </div>
                  <div className="text-[10px] text-gray-500 dark:text-gray-400 max-w-50 truncate leading-tight">
                    {t.tapp.listSubtitle}
                  </div>
                </div>

                {/* 鍒嗛殧绾? */}
                <div className="hidden sm:block h-6 w-px bg-gray-200 dark:bg-white/10 mx-1" />

                {/* 鎿嶄綔鎸夐挳 */}
                <div className="hidden sm:flex items-center gap-2">
                  {isAdmin && (
                    <button
                      onClick={() => {
                        void import('../../utils/analyticsEvents').then(
                          ({ trackProductEvent, AnalyticsEvents }) => {
                            trackProductEvent(AnalyticsEvents.TAPP_PLAYGROUND, {
                              throttleMs: 5000,
                            })
                          },
                        )
                        navigate(TAPP_PLAYGROUND_PATH)
                      }}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                      style={{ color: 'var(--color-primary)' }}
                      title={t.tapp.playgroundTitle}
                    >
                      <TappPlaygroundIcon className="w-3.5 h-3.5" />
                      {t.tapp.playground}
                    </button>
                  )}
                  {/* Regular user: toggle mine ↔ site-owner list (playground slot) */}
                  {canToggleListScope && (
                    <button
                      type="button"
                      onClick={handleToggleListScope}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                      style={{ color: 'var(--color-primary)' }}
                      title={
                        listScope === 'mine'
                          ? t.tapp.listScopeSwitchToSite
                          : t.tapp.listScopeSwitchToMine
                      }
                      aria-pressed={listScope === 'site'}
                    >
                      {listScope === 'mine' ? (
                        <FaGlobe className="w-3.5 h-3.5" />
                      ) : (
                        <FaUser className="w-3.5 h-3.5" />
                      )}
                      {listScope === 'mine'
                        ? t.tapp.listScopeSite
                        : t.tapp.listScopeMine}
                    </button>
                  )}
                  <button
                    onClick={() => navigate(TAPP_STORE_PATH)}
                    className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                    style={{ color: 'var(--color-primary)' }}
                  >
                    <MyriadStoreIcon className="w-4 h-4" />
                    {t.tapp.store}
                  </button>
                  {/* 多任务入口 - 仅平板和PC端显示，Safari 不支持 */}
                  {!isMobile && !isWebKit && (
                    <button
                      onClick={() => navigate(tappRunMultiPath())}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                      style={{ color: 'var(--color-primary)' }}
                      title={t.tapp.multiWindow}
                    >
                      <FaTh className="w-3 h-3" />
                      {t.tapp.multiWindow}
                    </button>
                  )}
                  {isAdmin && (
                    <button
                      onClick={(e) => openInstallDialog(e.currentTarget)}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10 cursor-pointer"
                      style={{ color: 'var(--color-primary)' }}
                      title={t.tapp.install}
                    >
                      <FaPlus className="w-3 h-3" />
                      {t.tapp.install}
                    </button>
                  )}
                </div>
              </div>

              {/* 右侧移动端按钮 — Playground 仅桌面端入口 */}
              <div className="flex sm:hidden items-center gap-2">
                {canToggleListScope && (
                  <button
                    type="button"
                    onClick={handleToggleListScope}
                    className="p-2 rounded-lg glass shadow-sm"
                    title={
                      listScope === 'mine'
                        ? t.tapp.listScopeSwitchToSite
                        : t.tapp.listScopeSwitchToMine
                    }
                    aria-pressed={listScope === 'site'}
                  >
                    {listScope === 'mine' ? (
                      <FaGlobe
                        className="w-5 h-5"
                        style={{ color: 'var(--color-primary)' }}
                      />
                    ) : (
                      <FaUser
                        className="w-5 h-5"
                        style={{ color: 'var(--color-primary)' }}
                      />
                    )}
                  </button>
                )}
                <button
                  onClick={() => navigate(TAPP_STORE_PATH)}
                  className="p-2 rounded-lg glass shadow-sm"
                  title={t.tapp.storeTitle}
                >
                  <MyriadStoreIcon
                    className="w-5 h-5"
                    style={{ color: 'var(--color-primary)' }}
                  />
                </button>
                {isAdmin && (
                  <button
                    onClick={(e) => openInstallDialog(e.currentTarget)}
                    className="p-2 rounded-lg glass shadow-sm"
                    title={t.tapp.manualInstall}
                  >
                    <FaPlus
                      className="w-5 h-5"
                      style={{ color: 'var(--color-primary)' }}
                    />
                  </button>
                )}
              </div>
            </div>
          </div>

          {/* Content — hold cards while public site layout hydrates (no order flash) */}
          {!listDisplayPending && orderedTapps.length === 0 && showEmpty ? (
            <div className="tapp-app-empty glass glass-chrome-free">
              <GlowBackground
                color="var(--color-primary, #6366f1)"
                animLevel={animConfig.level}
                shouldAnimate={animConfig.loop}
                variant="dual"
                size="lg"
                opacity={0.16}
              />
              <div className="tapp-app-empty__content">
                <div className="tapp-app-empty__icon">
                  <FaFolder className="w-9 h-9" />
                </div>
                <h3 className="tapp-app-empty__title">
                  {t.tapp.noAppsInstalled}
                </h3>
                <p className="tapp-app-empty__desc">
                  {t.tapp.noAppsInstalledDesc}
                </p>
                <div className="tapp-app-empty__actions">
                  <button
                    type="button"
                    onClick={() => navigate(TAPP_STORE_PATH)}
                    className="tapp-app-empty__btn tapp-app-empty__btn--primary"
                  >
                    <MyriadStoreIcon className="w-4 h-4" />
                    {t.tapp.browseStore}
                  </button>
                  {!isMobile && isAdmin && (
                    <button
                      type="button"
                      onClick={() => {
                        void import('../../utils/analyticsEvents').then(
                          ({ trackProductEvent, AnalyticsEvents }) => {
                            trackProductEvent(AnalyticsEvents.TAPP_PLAYGROUND, {
                              throttleMs: 5000,
                            })
                          },
                        )
                        navigate(TAPP_PLAYGROUND_PATH)
                      }}
                      className="tapp-app-empty__btn tapp-app-empty__btn--ghost"
                    >
                      <TappPlaygroundIcon className="w-4 h-4" />
                      {t.tapp.playground}
                    </button>
                  )}
                  {isAdmin && (
                    <button
                      type="button"
                      onClick={(e: ReactMouseEvent<HTMLButtonElement>) =>
                        openInstallDialog(e.currentTarget)
                      }
                      className="tapp-app-empty__btn tapp-app-empty__btn--ghost"
                      title={t.tapp.manualInstall}
                    >
                      <FaPlus className="w-4 h-4" />
                      {t.tapp.manualInstall}
                    </button>
                  )}
                </div>
              </div>
            </div>
          ) : (
            <div
              className={`tapp-app-card-grid${dragId ? ' is-reordering' : ''}`}
            >
              <AnimatePresence mode="popLayout">
                {!listDisplayPending &&
                  orderedTapps.map((tapp, index) => (
                    <TappAppCard
                      key={tapp.id}
                      tapp={tapp}
                      size={activeCardSizes[tapp.id] ?? '1x1'}
                      // Mobile: no reorder handle or 1x1↔2x1 size toggle
                      canResize={canEditLayout && !isMobile}
                      canReorder={canEditLayout && !isMobile}
                      dragLabel={t.tapp.cardDragReorder}
                      isDragging={dragId === tapp.id}
                      isDragOver={dragOverId === tapp.id && dragId !== tapp.id}
                      onDragHandleStart={handleDragHandleStart}
                      onDragOverCard={handleDragOverCard}
                      onDragLeaveCard={handleDragLeaveCard}
                      onDropOnCard={handleDropOnCard}
                      onDragEndCard={handleDragEndCard}
                      onToggleSize={
                        canEditLayout && !isMobile
                          ? () => handleToggleCardSize(tapp.id)
                          : undefined
                      }
                      isRunning={runningTapps.has(tapp.id)}
                      onStart={() => handleStart(tapp.id)}
                      onStop={() => handleStop(tapp.id)}
                      onUninstall={(anchor) => handleUninstall(tapp.id, anchor)}
                      onConfigure={() => handleConfigure(tapp.id)}
                      onOpen={() => handleOpen(tapp.id)}
                      index={index}
                    />
                  ))}
              </AnimatePresence>
            </div>
          )}
        </div>
      </div>

      {/* 手动安装浮层（与卸载确认同款锚定 tooltip） */}
      <InstallTappDialog
        isOpen={showInstallDialog}
        anchorEl={installAnchor}
        onCancel={cancelInstall}
        onInstall={() => loadTapps(true)}
        onSuccess={(name) =>
          showToastMessage(
            t.tapp.installSuccess.replace('{name}', name),
            'success',
          )
        }
      />

      {/* 卸载确认浮层 */}
      <UninstallConfirmDialog
        isOpen={showUninstallDialog}
        appName={uninstallTargetName}
        anchorEl={uninstallAnchor}
        onCancel={cancelUninstall}
        onConfirm={handleConfirmUninstall}
      />

      {/* Toast 提示 */}
      {toastMessage && (
        <Toast
          message={toastMessage}
          type={toastType}
          onClose={() => setToastMessage('')}
        />
      )}
    </AnimatedView>
  )
}

export default TappListPage
