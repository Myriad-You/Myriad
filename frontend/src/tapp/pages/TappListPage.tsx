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
import { userFacingError } from '../../utils/userFacingError'
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

export function TappListPage() {
  const navigate = useNavigate()
  const { t, locale, format } = useI18n()
  const { isMobile } = useBreakpoints()
  const { isAdmin, isAuthenticated, hasChecked, checkAuth } = useAuth()
  const animConfig = useAnimationLevel()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.tapp,
    { isAuthenticated: false, isAdmin: false },
  )
  const { currentFont, titleFontSize } = useTitleFont()
  const titleColorCss = useResolvedTitleColor()

  const [tapps, setTapps] = useState<TappInstance[]>([])
  /** null = 未加载；不要当成空或回退 runtime 过滤。 */
  const [siteTapps, setSiteTapps] = useState<TappInstance[] | null>(null)
  const [runningTapps, setRunningTapps] = useState<Set<string>>(new Set())
  /** 访客绝不从个人 localStorage 灌顺序。 */
  const [cardSizes, setCardSizes] = useState<Record<string, TappAppCardSize>>(
    () => (hasSessionHint() ? loadTappAppCardSizes() : {}),
  )
  const [cardOrder, setCardOrder] = useState<string[]>(
    () => (hasSessionHint() ? loadTappAppCardLayout().order : []),
  )
  const [siteCardSizes, setSiteCardSizes] = useState<
    Record<string, TappAppCardSize>
  >({})
  const [siteCardOrder, setSiteCardOrder] = useState<string[]>([])
  const [layoutReady, setLayoutReady] = useState(false)
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
  const dragSessionCleanupRef = useRef<(() => void) | null>(null)
  const suppressOpenRef = useRef(false)
  const [loading, setLoading] = useState(true)
  const [showEmpty, setShowEmpty] = useState(false)
  const [showInstallDialog, setShowInstallDialog] = useState(false)
  const [installAnchor, setInstallAnchor] = useState<HTMLElement | null>(null)
  const [toastMessage, setToastMessage] = useState('')
  const [toastType, setToastType] = useState<ToastType>('info')
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

  useTappScheduler()

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
        const needsReauthorization = detail.needs_reauthorization ?? false
        const isRunning =
          !needsReauthorization &&
          (runtime.isRunning(detail.id) || installationStatus === 'running')
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
          needsReauthorization,
          userRole: existing?.userRole ?? (isAdmin ? 'admin' : 'user'),
          isTemporary: false,
          isAdminTapp: true,
          visibility: detail.visibility === 'admin' ? 'admin' : 'all',
          error: detail.error_message,
        }
      })
    },
    [runtime, isAdmin],
  )

  const loadTapps = useCallback(
    async (forceSync: boolean = false) => {
      if (forceSync) {
        await runtime.syncFromBackend(true)
      }

      const allTapps = runtime.getAllTapps()
      setTapps(allTapps)
      setLoading(false)

      const running = new Set<string>()
      allTapps.forEach((tapp) => {
        if (runtime.isRunning(tapp.id)) {
          running.add(tapp.id)
        }
      })
      setRunningTapps(running)

      // 未就绪保持 null，site 范围不用 runtime 回退。
      if (isAuthenticated && !isAdmin) {
        try {
          const details = await listTappDetails('site')
          setSiteTapps(mapSiteDetails(details))
        } catch (error) {
          console.error('Failed to load site Tapp catalog:', error)
          showToastMessage(
            userFacingError(error, t.tapp.listLoadFailed),
            'error',
          )
          setSiteTapps((prev) => prev ?? [])
        }
      } else {
        setSiteTapps(null)
      }
    },
    [
      runtime,
      isAuthenticated,
      isAdmin,
      mapSiteDetails,
      showToastMessage,
      t.tapp.listLoadFailed,
    ],
  )

  const canToggleListScope = isAuthenticated && !isAdmin

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

  const siteCatalogPending =
    canToggleListScope && listScope === 'site' && siteTapps === null

  const scopedTapps = useMemo(() => {
    if (!canToggleListScope) return dedupeById(tapps)
    if (listScope === 'site') {
      if (siteTapps === null) return []
      return dedupeById(siteTapps)
    }
    return dedupeById(tapps.filter((t) => t.isAdminTapp !== true))
  }, [tapps, siteTapps, listScope, canToggleListScope, dedupeById])

  const useSiteLayout = canToggleListScope && listScope === 'site'
  const activeCardSizes = useSiteLayout ? siteCardSizes : cardSizes
  const activeCardOrder = useSiteLayout ? siteCardOrder : cardOrder

  /** 访客主视图与普通用户 site 范围：等站点布局就绪再画卡片。 */
  const siteLayoutPending = isSiteOwnerLayoutPending({
    layoutReady,
    isAuthenticated,
    isSiteScope: useSiteLayout,
  })

  const orderedTapps = useMemo(
    () => applyTappAppCardOrder(scopedTapps, activeCardOrder),
    [scopedTapps, activeCardOrder],
  )

  const listDisplayPending =
    loading || siteCatalogPending || siteLayoutPending

  const canEditLayout =
    isAuthenticated && (!canToggleListScope || listScope === 'mine')

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

    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }

    const initLoad = async () => {
      await runtime.waitForSync()
      if (mounted) {
        loadTapps()
      }
    }

    initLoad()

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
      showToastMessage(userFacingError(error, t.tapp.startAppFailed), 'error')
    }
  }

  const handleStop = async (tappId: string) => {
    try {
      await runtime.stopTapp(tappId)
    } catch (error) {
      console.error('Failed to stop Tapp:', error)
      showToastMessage(userFacingError(error, t.tapp.stopAppFailed), 'error')
    }
  }

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
        showToastMessage(
          userFacingError(error, t.tapp.uninstallFailed),
          'error',
        )
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

  // 不要把展示合并结果全量写回（会冻住站主尺寸）。
  useEffect(() => {
    if (!hasChecked) return
    let cancelled = false
    setLayoutReady(false)

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
          const localPersonalOnly: Record<string, TappAppCardSize> = {}
          for (const [id, size] of Object.entries(local.sizes)) {
            if (Object.hasOwn(remote.sizes, id)) continue
            if (remote.siteSizes[id] === size) continue
            localPersonalOnly[id] = size
          }
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
            await saveTappListCardSizes({
              sizes: personalSizes,
              order: personalOrder,
            })
          }
        } else {
          setCardSizes(remote.sizes)
          setCardOrder(remote.order)
        }
      } catch {
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

  const persistLayout = useCallback(
    (sizes: Record<string, TappAppCardSize>, order: string[]) => {
      saveTappAppCardLayout({ sizes, order })
      void saveTappListCardSizes({ sizes, order }).catch(() => {
      })
    },
    [],
  )

  const handleToggleCardSize = useCallback(
    (tappId: string) => {
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
      }
      return next
    })
  }, [])

  const clearCardDrag = useCallback(() => {
    dragIdRef.current = null
    setDragId(null)
    setDragOverId(null)
    const cleanup = dragSessionCleanupRef.current
    if (cleanup) {
      dragSessionCleanupRef.current = null
      cleanup()
    }
  }, [])

  const finishCardDrag = useCallback(() => {
    clearCardDrag()
    suppressOpenRef.current = true
    window.setTimeout(() => {
      suppressOpenRef.current = false
    }, 0)
  }, [clearCardDrag])

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
      dragSessionCleanupRef.current?.()
      dragSessionCleanupRef.current = null

      suppressOpenRef.current = true
      dragIdRef.current = tappId
      setDragId(tappId)
      setDragOverId(null)
      e.dataTransfer.effectAllowed = 'move'
      e.dataTransfer.setData('text/plain', tappId)

      const onWindowDragEnd = () => {
        finishCardDrag()
      }
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
        const base = applyTappAppCardOrder(scopedTapps, prev).map((t) => t.id)
        const fromIndex = base.indexOf(fromId)
        const toIndex = base.indexOf(toId)
        if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return prev
        const next = base
          .toSpliced(fromIndex, 1)
          .toSpliced(toIndex, 0, fromId)
        persistLayout(cardSizes, next)
        return next
      })
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
          <div className="hidden lg:block flex-1 min-h-[30vh]" />

          <div className="relative h-12 shrink-0 z-10">
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

                <div className="hidden sm:block h-6 w-px bg-gray-200 dark:bg-white/10 mx-1" />

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
                      data-tour="tapp-open-playground"
                    >
                      <TappPlaygroundIcon className="w-3.5 h-3.5" />
                      {t.tapp.playground}
                    </button>
                  )}
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
                      data-tour="tapp-scope"
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
                    data-tour="tapp-store-entry"
                  >
                    <MyriadStoreIcon className="w-4 h-4" />
                    {t.tapp.store}
                  </button>
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
                      data-tour="tapp-install"
                    >
                      <FaPlus className="w-3 h-3" />
                      {t.tapp.install}
                    </button>
                  )}
                </div>
              </div>

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
                    data-tour="tapp-scope"
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
                  data-tour="tapp-store-entry"
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
                    data-tour="tapp-install"
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

          <div
            data-tour="tapp-grid"
            data-tour-fit=".tapp-app-card, .tapp-app-empty"
          >
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
      </div>

      <InstallTappDialog
        isOpen={showInstallDialog}
        anchorEl={installAnchor}
        onCancel={cancelInstall}
        onInstall={() => loadTapps(true)}
        onSuccess={(name) =>
          showToastMessage(
            format(t.tapp.installSuccess, { name }),
            'success',
          )
        }
      />

      <UninstallConfirmDialog
        isOpen={showUninstallDialog}
        appName={uninstallTargetName}
        anchorEl={uninstallAnchor}
        onCancel={cancelUninstall}
        onConfirm={handleConfirmUninstall}
      />

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
