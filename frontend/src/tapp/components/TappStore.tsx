import type { CSSProperties } from 'react'
import type {
  RemoteApp,
  RemoteStoreSource,
} from '../services/RemoteStoreService'
import type { TappCategory } from '../types'
import type {
  CategorySortOrder,
  InstalledSortOrder,
  InstalledTappInfo,
  StoreSelection,
  TappStoreProps,
  UnifiedAppItem,
} from './store'
import {
  FaArrowLeft,
  FaCog,
  FaCompass,
  FaSearch,
  FaStar,
  FaSync,
  FaTimesCircle,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {

  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useNavigate } from 'react-router-dom'
import { Spinner } from '../../components/Spinner'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { ensureMotionReady, isMotionReady } from '../../lib/lazyMotion'
import { hasSessionHint } from '../../utils/sessionDetection'
import { showError, showInfo, showSuccess } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { EXAMPLE_TAPPS } from '../examples'
import { getTappRuntime } from '../runtime'
import { RemoteStoreService } from '../services/RemoteStoreService'
import { resolveManifestText } from '../utils/manifestLocale'
import { selectFeaturedStoreApps } from '../utils/storeCatalogState'
import { resolveStoreMerchandising } from '../utils/storeLocale'
import {
  normalizeTappCategory,
  TAPP_CATEGORIES,
  TAPP_CATEGORY_I18N_KEYS,
} from '../utils/tappCategories'
import { tappRunPath } from '../utils/tappPaths'
import {
  compareVersions,
  findStoreSource,
} from '../utils/tappStoreHelpers'
import {
  AppDetailView,
  CATEGORY_ICONS,
  CategoryPill,
  DetailHeaderActions,
  DISCOVER_LATEST_LIMIT,
  StoreCatalogView,
  StoreConfigurationView,
  UnifiedAppCard,
} from './store'
import { UninstallConfirmDialog } from './UninstallConfirmDialog'
import '../../components/ConfigForm.css'
import './TappStore.css'

export type { TappStoreProps } from './store'

export function TappStore({
  onInstalled,
  className = '',
  embeddedChrome = false,
  compact = false,
  fullscreen = false,
}: TappStoreProps) {
  const { t, locale, format } = useI18n()
  const navigate = useNavigate()
  const { isAuthenticated, isAdmin, hasChecked, checkAuth } = useAuth()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<StoreSelection>(null)
  const [installedSortOrder, setInstalledSortOrder] =
    useState<InstalledSortOrder>('category')
  const [categorySortOrder, setCategorySortOrder] =
    useState<CategorySortOrder>('name')
  const [detailApp, setDetailApp] = useState<UnifiedAppItem | null>(null)
  const [showStoreConfiguration, setShowStoreConfiguration] = useState(false)
  const [showAllAppsPage, setShowAllAppsPage] = useState(false)
  const [installedTapps, setInstalledTapps] = useState<
    Map<string, InstalledTappInfo>
  >(new Map())
  const [installingIds, setInstallingIds] = useState(() => new Set<string>())
  const [updatingIds, setUpdatingIds] = useState(() => new Set<string>())
  const [installProgressById, setInstallProgressById] = useState(
    () =>
      new Map<
        string,
        { percent: number; phase: string; detail?: string }
      >(),
  )
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const [uninstallTargetId, setUninstallTargetId] = useState<string | null>(
    null,
  )
  const [uninstallTargetName, setUninstallTargetName] = useState('')
  const [uninstallAnchor, setUninstallAnchor] = useState<HTMLElement | null>(
    null,
  )

  const animConfig = useAnimationLevel()
  const [motionReady, setMotionReady] = useState(isMotionReady)
  useEffect(() => {
    if (motionReady) return
    let cancelled = false
    void ensureMotionReady().then(() => {
      if (!cancelled) setMotionReady(true)
    })
    return () => {
      cancelled = true
    }
  }, [motionReady])

  const [remoteApps, setRemoteApps] = useState<
    Array<
      RemoteApp & {
        sourceUrl: string
        sourceName: string
        sourceBaseUrl: string
        sourceOfficial?: boolean
      }
    >
  >([])
  const [sources, setSources] = useState<RemoteStoreSource[]>([])

  const runtime = getTappRuntime()
  const notifyInstalled = useCallback(() => {
    onInstalled?.()
  }, [onInstalled])

  const installedIds = useMemo(
    () => new Set(installedTapps.keys()),
    [installedTapps],
  )

  const loadInstalledTapps = useCallback(() => {
    const allTapps = runtime.getAllTapps()
    const tappsMap = new Map<string, InstalledTappInfo>()
    allTapps.forEach((tapp) => {
      tappsMap.set(tapp.id, {
        userRole: tapp.userRole,
        isTemporary: tapp.isTemporary,
        version: tapp.manifest.version,
        installedAt: tapp.installedAt,
      })
    })
    setInstalledTapps(tappsMap)
  }, [runtime])

  useEffect(() => {
    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }
  }, [hasChecked, checkAuth])

  useEffect(() => {
    let mounted = true

    const initLoad = async () => {
      await runtime.waitForSync()
      if (mounted) {
        loadInstalledTapps()
      }
    }

    initLoad()

    const unsubscribe = runtime.on('sync:complete', () => {
      if (mounted) {
        loadInstalledTapps()
      }
    })

    return () => {
      mounted = false
      unsubscribe()
    }
  }, [runtime, loadInstalledTapps])

  useEffect(() => {
    const loadSources = async () => {
      const loadedSources = await RemoteStoreService.getSources()
      setSources(loadedSources)
    }
    loadSources()
  }, [])

  const loadRemoteGenRef = useRef(0)
  const loadRemoteApps = useCallback(async (forceRefresh = false) => {
    const gen = ++loadRemoteGenRef.current
    setLoading(true)
    setError(null)
    try {
      const result = await RemoteStoreService.fetchAllApps(forceRefresh)
      if (gen !== loadRemoteGenRef.current) return
      setRemoteApps(result.apps)

      const errors = result.sources.filter((s) => s.error)
      if (errors.length > 0) {
        setError(userFacingError(errors[0].error, t.tapp.loadRemoteFailed))
      }
    } catch (err) {
      if (gen !== loadRemoteGenRef.current) return
      setError(
        userFacingError(err, t.tapp.loadRemoteFailed),
      )
    } finally {
      if (gen === loadRemoteGenRef.current) setLoading(false)
    }
  }, [t])

  useEffect(() => {
    if (remoteApps.length === 0) {
      loadRemoteApps()
    }
  }, [loadRemoteApps, remoteApps.length])

  const localApps: UnifiedAppItem[] = useMemo(
    () =>
      EXAMPLE_TAPPS.map((tapp) => {
        const text = resolveManifestText(tapp.manifest, locale)
        return {
          id: tapp.manifest.id,
          name: text.name,
          version: tapp.manifest.version,
          description: text.description || '',
          author: tapp.manifest.author || { name: 'Unknown' },
          icon: tapp.manifest.icon,
          iconSvg: tapp.manifest.iconSvg,
          iconShell: tapp.manifest.iconShell,
          themeColor: tapp.manifest.themeColor,
          category: tapp.manifest.category,
          tags: tapp.tags,
          permissions: tapp.manifest.permissions,
          source: 'local' as const,
          localTapp: tapp,
        }
      }),
    [locale],
  )

  const remoteAppsUnified: UnifiedAppItem[] = useMemo(
    () =>
      remoteApps.map((app) => {
        const merch = resolveStoreMerchandising(app, locale)
        return {
          id: app.id,
          name: merch.name,
          version: app.version,
          description: merch.description || '',
          longDescription: merch.longDescription,
          preview: merch.preview,
          author: app.author,
          icon: app.icon,
          iconSvg: app.icon_svg,
          iconShell: app.icon_shell,
          themeColor: app.theme_color,
          category: normalizeTappCategory(app.category),
          tags: app.tags || [],
          permissions: app.permissions,
          license: app.license,
          homepage: app.homepage,
          repository: app.repository,
          size: app.size,
          downloads:
            typeof app.downloads === 'number' && app.downloads > 0
              ? app.downloads
              : undefined,
          featured: app.featured,
          verified: app.verified,
          fromOfficialSource: Boolean(app.sourceOfficial),
          updatedAt: app.updated_at,
          source: 'remote' as const,
          remoteApp: app,
        }
      }),
    [remoteApps, locale],
  )

  // 首屏远程未落地前不并入内置示例。
  const catalogSettled = !loading || remoteApps.length > 0 || error != null

  const allApps: UnifiedAppItem[] = useMemo(() => {
    const merged: UnifiedAppItem[] = Iterator.from(remoteAppsUnified).toArray()
    if (catalogSettled) {
      for (const localApp of localApps) {
        if (!merged.some((r) => r.id === localApp.id)) {
          merged.push(localApp)
        }
      }
    }
    for (const [id, info] of installedTapps) {
      if (merged.some((a) => a.id === id)) continue
      const instance = runtime.getTapp(id)
      if (!instance?.manifest) continue
      const text = resolveManifestText(instance.manifest, locale)
      merged.push({
        id,
        name: text.name,
        version: info.version || instance.manifest.version,
        description: text.description || '',
        author: instance.manifest.author || { name: 'Unknown' },
        icon: instance.manifest.icon,
        iconSvg: instance.manifest.iconSvg,
        iconShell: instance.manifest.iconShell,
        themeColor: instance.manifest.themeColor,
        category: normalizeTappCategory(instance.manifest.category),
        tags: [],
        permissions: instance.manifest.permissions || [],
        source: 'local' as const,
        updatedAt: info.installedAt,
      })
    }
    return merged
  }, [
    remoteAppsUnified,
    localApps,
    installedTapps,
    runtime,
    locale,
    catalogSettled,
  ])

  const detailAppId = detailApp?.id ?? null
  useEffect(() => {
    if (!detailAppId) return
    const next = allApps.find((a) => a.id === detailAppId)
    if (!next) {
      if (!installedIds.has(detailAppId)) setDetailApp(null)
      return
    }
    setDetailApp((prev) => {
      if (!prev || prev.id !== next.id) return prev
      if (
        prev.version === next.version &&
        prev.name === next.name &&
        prev.description === next.description &&
        prev.longDescription === next.longDescription &&
        prev.preview === next.preview &&
        prev.updatedAt === next.updatedAt &&
        prev.source === next.source &&
        prev.remoteApp === next.remoteApp &&
        prev.localTapp === next.localTapp &&
        prev.permissions === next.permissions
      ) {
        return prev
      }
      return next
    })
  }, [allApps, detailAppId, installedIds])

  const availableUpdates = allApps.filter((app) => {
    const installed = installedTapps.get(app.id)
    return !!installed && compareVersions(app.version, installed.version) > 0
  })
  const availableUpdateIds = new Set(availableUpdates.map((app) => app.id))

  const parseDate = (value?: string) => {
    if (!value) return 0
    const timestamp = Date.parse(value)
    return Number.isFinite(timestamp) ? timestamp : 0
  }

  const filteredAppsUnsorted = allApps.filter((app) => {
    if (searchQuery) {
      const query = searchQuery.toLowerCase()
      const matchName = app.name.toLowerCase().includes(query)
      const matchDesc = app.description.toLowerCase().includes(query)
      const matchLong = app.longDescription?.toLowerCase().includes(query)
      const matchTags = app.tags.some((t) => t.toLowerCase().includes(query))
      const remote = app.remoteApp
      const matchRaw =
        !!remote &&
        (remote.name.toLowerCase().includes(query) ||
          remote.description.toLowerCase().includes(query) ||
          (remote.long_description?.toLowerCase().includes(query) ?? false) ||
          Object.values(remote.locales ?? {}).some(
            (entry) =>
              (entry.name?.toLowerCase().includes(query) ?? false) ||
              (entry.description?.toLowerCase().includes(query) ?? false) ||
              (entry.long_description?.toLowerCase().includes(query) ?? false),
          ))
      if (!matchName && !matchDesc && !matchLong && !matchTags && !matchRaw)
        return false
    }
    if (selectedCategory === '__installed__') {
      return installedIds.has(app.id)
    }
    if (selectedCategory && app.category !== selectedCategory) return false
    return true
  })

  const compareCatalogSort = (a: UnifiedAppItem, b: UnifiedAppItem) => {
    if (categorySortOrder === 'date') {
      const dateOrder = parseDate(b.updatedAt) - parseDate(a.updatedAt)
      if (dateOrder !== 0) return dateOrder
    } else if (categorySortOrder === 'downloads') {
      const downloadOrder = (b.downloads ?? 0) - (a.downloads ?? 0)
      if (downloadOrder !== 0) return downloadOrder
    }
    return a.name.localeCompare(b.name, locale)
  }

  const filteredApps =
    selectedCategory === '__installed__'
      ? filteredAppsUnsorted.toSorted((a, b) => {
          const categoryOrder =
            TAPP_CATEGORIES.indexOf(a.category) -
            TAPP_CATEGORIES.indexOf(b.category)
          const dateOrder =
            parseDate(installedTapps.get(b.id)?.installedAt) -
            parseDate(installedTapps.get(a.id)?.installedAt)

          if (installedSortOrder === 'category') {
            if (categoryOrder !== 0) return categoryOrder
            if (dateOrder !== 0) return dateOrder
          } else {
            if (dateOrder !== 0) return dateOrder
            if (categoryOrder !== 0) return categoryOrder
          }
          return a.name.localeCompare(b.name, locale)
        })
      : selectedCategory
        ? filteredAppsUnsorted.toSorted(compareCatalogSort)
        : filteredAppsUnsorted

  const allAppsCatalogSorted = useMemo(() => {
    return allApps.toSorted((a, b) => {
      if (categorySortOrder === 'date') {
        const dateOrder = parseDate(b.updatedAt) - parseDate(a.updatedAt)
        if (dateOrder !== 0) return dateOrder
      } else if (categorySortOrder === 'downloads') {
        const downloadOrder = (b.downloads ?? 0) - (a.downloads ?? 0)
        if (downloadOrder !== 0) return downloadOrder
      }
      return a.name.localeCompare(b.name, locale)
    })
  }, [allApps, categorySortOrder, locale])

  const installedCurrentApps =
    selectedCategory === '__installed__'
      ? Iterator.from(filteredApps)
          .filter((app) => !availableUpdateIds.has(app.id))
          .toArray()
      : []
  const sortedAvailableUpdates =
    selectedCategory === '__installed__'
      ? Iterator.from(filteredApps)
          .filter((app) => availableUpdateIds.has(app.id))
          .toArray()
          .toSorted((a, b) => {
            const dateOrder =
              parseDate(b.updatedAt ?? installedTapps.get(b.id)?.installedAt) -
              parseDate(a.updatedAt ?? installedTapps.get(a.id)?.installedAt)
            if (dateOrder !== 0) return dateOrder
            return a.name.localeCompare(b.name, locale)
          })
      : []

  const handleUninstall = useCallback(
    (appId: string, anchor?: HTMLElement | null) => {
      const app = filteredApps.find((a) => a.id === appId)
      setUninstallTargetId(appId)
      setUninstallTargetName(app?.name || appId)
      setUninstallAnchor(anchor ?? null)
      setShowUninstallDialog(true)
    },
    [filteredApps],
  )

  const handleConfirmUninstall = useCallback(
    async (keepData: boolean) => {
      if (!uninstallTargetId) return
      try {
        await runtime.uninstallTapp(uninstallTargetId, { keepData })
        setInstalledTapps((prev) => {
          const next = new Map(prev)
          next.delete(uninstallTargetId)
          return next
        })
        notifyInstalled()
        setShowUninstallDialog(false)
        setUninstallTargetId(null)
        setUninstallAnchor(null)
        showSuccess(
          uninstallTargetName
            ? `${t.tapp.uninstall}: ${uninstallTargetName}`
            : t.tapp.uninstall,
        )
      } catch (error) {
        console.error('Failed to uninstall Tapp:', error)
        showError(
          userFacingError(error, t.tapp.uninstallFailed),
          t.tapp.uninstallFailed,
        )
        throw error
      }
    },
    [runtime, notifyInstalled, uninstallTargetId, uninstallTargetName, t],
  )

  const cancelUninstall = useCallback(() => {
    setShowUninstallDialog(false)
    setUninstallTargetId(null)
    setUninstallAnchor(null)
  }, [])

  const patchProgress = useCallback(
    (
      appId: string,
      patch: { percent: number; phase: string; detail?: string },
    ) => {
      setInstallProgressById((prev) => {
        const next = new Map(prev)
        next.set(appId, patch)
        return next
      })
    },
    [],
  )

  const clearProgress = useCallback((appId: string) => {
    setInstallProgressById((prev) => {
      if (!prev.has(appId)) return prev
      const next = new Map(prev)
      next.delete(appId)
      return next
    })
  }, [])

  const handleInstall = useCallback(
    async (app: UnifiedAppItem) => {
      // 以 checkAuth 返回值为准；超时/5xx 也不得安装。
      let authed = isAuthenticated
      if (!authed) {
        if (!hasChecked || hasSessionHint()) {
          authed = await checkAuth()
        }
        if (!authed) {
          showInfo(t.tapp.loginRequiredToInstall)
          return
        }
      }
      if (installingIds.has(app.id) || updatingIds.has(app.id)) return

      setInstallingIds((prev) => new Set(prev).add(app.id))
      clearProgress(app.id)
      try {
        if (app.source === 'local' && app.localTapp) {
          await runtime.installTapp(app.localTapp.manifest, app.localTapp.code)
        } else if (app.source === 'remote' && app.remoteApp) {
          let source = findStoreSource(sources, app.remoteApp.sourceUrl)
          if (!source) {
            const latest = await RemoteStoreService.getSources()
            source = findStoreSource(latest, app.remoteApp.sourceUrl)
            if (latest.length) setSources(latest)
          }
          const sourceRef =
            (source?.id != null && String(source.id) !== ''
              ? String(source.id)
              : null) ||
            source?.url ||
            app.remoteApp.sourceUrl
          if (!sourceRef) {
            throw new Error(t.tapp.loadRemoteFailed)
          }

          const { installFromStore } =
            await import('../services/TappApiService')
          const { isLargeTappInstall, clampInstallPercent } =
            await import('../utils/tappInstallProgress')
          const estimatedBytes = app.size ?? app.remoteApp.size ?? 0
          const showProgress = isLargeTappInstall(estimatedBytes)

          await installFromStore(
            {
              source: sourceRef,
              tappId: app.id,
              permissions: app.permissions,
            },
            {
              estimatedBytes,
              onProgress: showProgress
                ? (p) => {
                    patchProgress(app.id, {
                      percent: clampInstallPercent(p.percent ?? 0),
                      phase: p.phase || p.message,
                      detail: p.detail,
                    })
                  }
                : undefined,
            },
          )

          await runtime.syncFromBackend(true)
        } else {
          throw new Error(t.tapp.installFailed)
        }

        const installed = runtime.getTapp(app.id)
        if (!installed) {
          throw new Error(t.tapp.installFailed)
        }
        setInstalledTapps(
          (prev) =>
            new Map([
              ...prev,
              [
                app.id,
                {
                  userRole: installed.userRole,
                  isTemporary: installed.isTemporary,
                  version: installed.manifest.version,
                  installedAt: installed.installedAt,
                },
              ],
            ]),
        )
        notifyInstalled()
        showSuccess(format(t.tapp.installSuccess, { name: app.name }))
      } catch (error) {
        console.error('Failed to install Tapp:', error)
        showError(
          userFacingError(error, t.tapp.installFailed),
          t.tapp.installFailed,
        )
      } finally {
        setInstallingIds((prev) => {
          const next = new Set(prev)
          next.delete(app.id)
          return next
        })
        clearProgress(app.id)
      }
    },
    [
      runtime,
      notifyInstalled,
      sources,
      t,
      isAuthenticated,
      hasChecked,
      checkAuth,
      installingIds,
      updatingIds,
      patchProgress,
      clearProgress,
    ],
  )

  const handleUpdate = useCallback(
    async (app: UnifiedAppItem) => {
      let authed = isAuthenticated
      if (!authed) {
        if (!hasChecked || hasSessionHint()) {
          authed = await checkAuth()
        }
        if (!authed) {
          showInfo(t.tapp.loginRequiredToInstall)
          return
        }
      }
      if (installingIds.has(app.id) || updatingIds.has(app.id)) return

      setUpdatingIds((prev) => new Set(prev).add(app.id))
      clearProgress(app.id)
      try {
        if (app.source === 'local' && app.localTapp) {
          const { updateTappFromCode } =
            await import('../services/TappApiService')
          await updateTappFromCode(app.localTapp.manifest, app.localTapp.code)
        } else if (app.source === 'remote' && app.remoteApp) {
          let source = findStoreSource(sources, app.remoteApp.sourceUrl)
          if (!source) {
            const latest = await RemoteStoreService.getSources()
            source = findStoreSource(latest, app.remoteApp.sourceUrl)
            if (latest.length) setSources(latest)
          }
          const sourceRef =
            (source?.id != null && String(source.id) !== ''
              ? String(source.id)
              : null) ||
            source?.url ||
            app.remoteApp.sourceUrl
          if (!sourceRef) {
            throw new Error(t.tapp.loadRemoteFailed)
          }

          const { updateTappFromStore } =
            await import('../services/TappApiService')
          const { isLargeTappInstall, clampInstallPercent } =
            await import('../utils/tappInstallProgress')
          const estimatedBytes = app.size ?? app.remoteApp?.size ?? 0
          const showProgress = isLargeTappInstall(estimatedBytes)

          await updateTappFromStore(
            app.id,
            { source: sourceRef },
            {
              estimatedBytes,
              onProgress: showProgress
                ? (p) => {
                    patchProgress(app.id, {
                      percent: clampInstallPercent(p.percent ?? 0),
                      phase: p.phase || p.message,
                      detail: p.detail,
                    })
                  }
                : (p) => {
                    if (p.percent != null && p.percent > 0) {
                      patchProgress(app.id, {
                        percent: clampInstallPercent(p.percent ?? 0),
                        phase: p.phase || p.message,
                        detail: p.detail,
                      })
                    }
                  },
            },
          )
          runtime.clearCodeCache(app.id)
        } else {
          throw new Error(t.tapp.updateFailed)
        }

        await runtime.refreshTapp(app.id)

        setInstalledTapps((prev) => {
          const newMap = new Map(prev)
          const existing = prev.get(app.id)
          if (existing) {
            newMap.set(app.id, { ...existing, version: app.version })
          }
          return newMap
        })
        notifyInstalled()
        showSuccess(t.tapp.updateSuccess)
      } catch (error) {
        console.error('Failed to update Tapp:', error)
        showError(
          userFacingError(error, t.tapp.updateFailed),
          t.tapp.updateFailed,
        )
      } finally {
        setUpdatingIds((prev) => {
          const next = new Set(prev)
          next.delete(app.id)
          return next
        })
        clearProgress(app.id)
      }
    },
    [
      runtime,
      notifyInstalled,
      sources,
      t,
      isAuthenticated,
      hasChecked,
      checkAuth,
      installingIds,
      updatingIds,
      patchProgress,
      clearProgress,
    ],
  )

  const handleToggleSource = async (url: string, enabled: boolean) => {
    const source = findStoreSource(sources, url)
    if (!source?.id) {
      showError(t.tapp.loadRemoteFailed)
      return
    }
    try {
      await RemoteStoreService.toggleSource(source.id, enabled)
      const updatedSources = await RemoteStoreService.getSources()
      setSources(updatedSources)
      // 启用/禁用后必须重拉目录。
      await loadRemoteApps(true)
    } catch (error) {
      console.error('Failed to toggle source:', error)
      showError(userFacingError(error, t.tapp.toggleSourceFailed))
    }
  }

  const handleRemoveSource = async (url: string) => {
    const source = findStoreSource(sources, url)
    if (!source?.id) {
      const err = new Error(t.tapp.loadRemoteFailed)
      showError(err.message)
      throw err
    }
    try {
      await RemoteStoreService.removeSource(source.id)
      const updatedSources = await RemoteStoreService.getSources()
      setSources(updatedSources)
      await loadRemoteApps(true)
      showSuccess(t.tapp.deleteSource)
    } catch (error) {
      console.error('Failed to remove source:', error)
      showError(userFacingError(error, t.tapp.deleteSourceFailed))
      throw error
    }
  }

  const handleAddSource = async (
    source: Omit<RemoteStoreSource, 'id' | 'official'>,
  ) => {
    try {
      await RemoteStoreService.addSource(source)
      const updatedSources = await RemoteStoreService.getSources()
      setSources(updatedSources)
      await loadRemoteApps(true)
      showSuccess(t.tapp.addSource)
    } catch (error) {
      console.error('Failed to add source:', error)
      showError(userFacingError(error, t.tapp.addSourceFailed))
      throw error
    }
  }

  const handleUpdateSource = async (
    source: RemoteStoreSource,
    patch: { name: string; url: string },
  ) => {
    if (source.official) {
      const err = new Error(t.tapp.updateSourceFailed)
      showError(err.message)
      throw err
    }
    if (!source.id) {
      const err = new Error(t.tapp.loadRemoteFailed)
      showError(err.message)
      throw err
    }
    try {
      await RemoteStoreService.updateSource(source.id, {
        name: patch.name,
        url: patch.url,
      })
      const updatedSources = await RemoteStoreService.getSources()
      setSources(updatedSources)
      await loadRemoteApps(true)
      showSuccess(t.tapp.saveSource)
    } catch (error) {
      console.error('Failed to update source:', error)
      showError(userFacingError(error, t.tapp.updateSourceFailed))
      throw error
    }
  }

  const categoryCounts = new Map<TappCategory, number>()

  for (const app of allApps) {
    categoryCounts.set(
      app.category,
      (categoryCounts.get(app.category) ?? 0) + 1,
    )
  }

  const categories = TAPP_CATEGORIES.flatMap((id) => {
    const count = categoryCounts.get(id)
    return count
      ? [{ id, name: t.tapp[TAPP_CATEGORY_I18N_KEYS[id]], count }]
      : []
  })

  const detailTappInfo = detailApp
    ? installedTapps.get(detailApp.id)
    : undefined
  const detailCanUninstall = detailTappInfo
    ? detailTappInfo.userRole === 'admin' ||
      (detailTappInfo.userRole === 'user' &&
        detailTappInfo.isTemporary === true)
    : false

  const listViewRef = useRef<HTMLDivElement | null>(null)
  const listScrollPosRef = useRef(0)
  const attachListView = useCallback((el: HTMLDivElement | null) => {
    listViewRef.current = el
    if (el) el.scrollTop = listScrollPosRef.current
  }, [])
  const attachDetailView = useCallback((el: HTMLDivElement | null) => {
    if (el) el.scrollTop = 0
  }, [])
  const openDetail = useCallback((app: UnifiedAppItem) => {
    listScrollPosRef.current = listViewRef.current?.scrollTop ?? 0
    setShowStoreConfiguration(false)
    setDetailApp(app)
  }, [])
  const openStoreConfiguration = useCallback(() => {
    listScrollPosRef.current = listViewRef.current?.scrollTop ?? 0
    setDetailApp(null)
    setShowAllAppsPage(false)
    setShowStoreConfiguration(true)
  }, [])
  const openAllAppsPage = useCallback(() => {
    listScrollPosRef.current = listViewRef.current?.scrollTop ?? 0
    setDetailApp(null)
    setShowStoreConfiguration(false)
    setShowAllAppsPage(true)
  }, [])
  const closeSecondaryView = useCallback(() => {
    if (detailApp) {
      setDetailApp(null)
      return
    }
    setShowStoreConfiguration(false)
    setShowAllAppsPage(false)
  }, [detailApp])

  const launchApp = useCallback(
    (appId: string) => {
      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.TAPP_RUN, {
            target: appId,
            throttleMs: 2000,
          })
        },
      )
      navigate(tappRunPath(appId))
    },
    [navigate],
  )

  // 避免 motionShim 把 opacity:0 固化成静态样式。
  const viewMotionProps = useCallback(
    (dir: 1 | -1) => {
      if (isExlight(animConfig) || !motionReady) {
        return { initial: false as const }
      }
      const scale = animConfig.durationScale
      const enterX = 18 * dir
      return {
        initial: { opacity: 0, x: enterX },
        animate: {
          opacity: 1,
          x: 0,
          transition: {
            duration: 0.3 * scale,
            ease: [0.22, 1, 0.36, 1] as const,
          },
        },
        exit: {
          opacity: 0,
          x: -14 * dir,
          transition: {
            duration: 0.18 * scale,
            ease: [0.4, 0, 1, 1] as const,
          },
        },
      }
    },
    [animConfig, motionReady],
  )

  const catalogMotionKey = useMemo(
    () =>
      `${selectedCategory ?? 'discover'}|${searchQuery.trim().toLowerCase()}`,
    [selectedCategory, searchQuery],
  )

  const isDiscoverView = !searchQuery && selectedCategory === null

  const featuredApps = useMemo(
    () => selectFeaturedStoreApps(allApps, isDiscoverView),
    [allApps, isDiscoverView],
  )

  const latestApps = useMemo(() => {
    if (!isDiscoverView) return []
    const parseDate = (value?: string) => {
      if (!value) return 0
      const timestamp = Date.parse(value)
      return Number.isFinite(timestamp) ? timestamp : 0
    }
    return allApps
      .toSorted((a, b) => {
        const dateOrder = parseDate(b.updatedAt) - parseDate(a.updatedAt)
        if (dateOrder !== 0) return dateOrder
        return a.name.localeCompare(b.name, locale)
      })
      .slice(0, DISCOVER_LATEST_LIMIT)
  }, [allApps, isDiscoverView, locale])

  const sectionTitle = useMemo(() => {
    if (selectedCategory === '__installed__') return t.tapp.installed
    if (selectedCategory) {
      return t.tapp[TAPP_CATEGORY_I18N_KEYS[selectedCategory]]
    }
    if (searchQuery) return t.tapp.searchApps.replace('...', '')
    return t.tapp.storeDiscover
  }, [selectedCategory, searchQuery, t])

  const selectCategory = useCallback((category: StoreSelection) => {
    setSearchQuery('')
    setSelectedCategory(category)
    setDetailApp(null)
    setShowStoreConfiguration(false)
    setShowAllAppsPage(false)
  }, [])

  const renderAppCard = (app: UnifiedAppItem, index: number, date?: string) => {
    const tappInfo = installedTapps.get(app.id)
    const canUpdate =
      !!tappInfo &&
      ((app.source === 'remote' && !!app.remoteApp) ||
        (app.source === 'local' && !!app.localTapp))

    return (
      <UnifiedAppCard
        key={app.id}
        app={app}
        isInstalled={installedIds.has(app.id)}
        installedVersion={tappInfo?.version}
        date={date}
        onInstall={() => handleInstall(app)}
        onUpdate={canUpdate ? () => handleUpdate(app) : undefined}
        onOpen={() => openDetail(app)}
        onLaunch={() => launchApp(app.id)}
        installing={installingIds.has(app.id)}
        installPercent={
          installProgressById.get(app.id)?.percent ?? null
        }
        installPhase={installProgressById.get(app.id)?.phase ?? null}
        installDetail={installProgressById.get(app.id)?.detail ?? null}
        updating={updatingIds.has(app.id)}
        index={index}
      />
    )
  }

  return (
    <div
      className={`as-store ${className}`}
      data-no-ripple
      data-store-compact={compact ? 'true' : undefined}
      data-embedded={embeddedChrome ? 'true' : undefined}
      data-fullscreen={fullscreen ? 'true' : undefined}
      data-store-anim={isExlight(animConfig) ? 'off' : 'on'}
      style={
        {
          ['--as-dur-scale' as string]: String(animConfig.durationScale),
        } as CSSProperties
      }
    >
      <div className="as-store__workspace">
        <aside
          className="as-store__sidebar glass glass-chrome-free"
          aria-label={t.tapp.storeBrowse}
        >
          <div className="as-store__sidebar-search as-store__search-shell">
            <FaSearch className="as-store__search-icon" />
            <input
              type="search"
              autoComplete="off"
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder={t.tapp.searchApps}
              className="as-store__search-input"
            />
            {searchQuery && (
              <button
                type="button"
                className="as-store__search-clear"
                onClick={() => setSearchQuery('')}
                title={t.tapp.clearSearch}
                aria-label={t.tapp.clearSearch}
              >
                <FaTimesCircle className="h-3.5 w-3.5" />
              </button>
            )}
          </div>

          <nav className="as-store__sidebar-nav">
            <p className="as-store__nav-heading">{t.tapp.storeLibrary}</p>
            <button
              type="button"
              className="as-store__nav-item"
              data-active={isDiscoverView ? 'true' : 'false'}
              onClick={() => selectCategory(null)}
            >
              <FaCompass />
              <span>{t.tapp.storeDiscover}</span>
            </button>
            <button
              type="button"
              className="as-store__nav-item"
              data-active={
                !searchQuery && selectedCategory === '__installed__'
                  ? 'true'
                  : 'false'
              }
              onClick={() => selectCategory('__installed__')}
            >
              <FaStar />
              <span>{t.tapp.installed}</span>
              <span className="as-store__nav-count">{installedIds.size}</span>
            </button>
            <p className="as-store__nav-heading">{t.tapp.storeBrowse}</p>
            {categories.map((category) => (
              <button
                key={category.id}
                type="button"
                className="as-store__nav-item"
                data-active={
                  !searchQuery && selectedCategory === category.id
                    ? 'true'
                    : 'false'
                }
                onClick={() => selectCategory(category.id)}
              >
                {CATEGORY_ICONS[category.id]}
                <span>{category.name}</span>
                <span className="as-store__nav-count">{category.count}</span>
              </button>
            ))}
          </nav>

          <div className="as-store__sidebar-tools">
            <button
              type="button"
              className="as-store__nav-item"
              onClick={() => loadRemoteApps(true)}
              disabled={loading}
              title={t.tapp.refreshStore}
              aria-label={t.tapp.refreshStore}
              aria-busy={loading || undefined}
            >
              {loading ? (
                <Spinner size="sm" color="current" />
              ) : (
                <FaSync />
              )}
              <span>{t.tapp.refreshStore}</span>
            </button>
            {isAdmin && (
              <button
                type="button"
                className="as-store__nav-item"
                onClick={openStoreConfiguration}
              >
                <FaCog />
                <span>{t.tapp.storeConfiguration}</span>
              </button>
            )}
          </div>
        </aside>

        <main className="as-store__main">
          <div className="as-store__chrome">
            <AnimatePresence mode="wait" initial={false}>
              {detailApp || showStoreConfiguration || showAllAppsPage ? (
                <motion.div
                  key={
                    detailApp
                      ? 'detail-chrome'
                      : showStoreConfiguration
                        ? 'configuration-chrome'
                        : 'all-apps-chrome'
                  }
                  {...viewMotionProps(1)}
                  className="as-store__back-bar"
                >
                  <button
                    type="button"
                    className="as-store__back as-detail__fluid-control glass glass-liquid"
                    onClick={closeSecondaryView}
                    aria-label={t.tapp.back}
                    title={t.tapp.back}
                  >
                    <FaArrowLeft className="h-3.5 w-3.5" />
                  </button>
                  {detailApp && <DetailHeaderActions app={detailApp} />}
                </motion.div>
              ) : (
                <motion.div
                  key="list-chrome"
                  {...viewMotionProps(-1)}
                  className="as-store__chrome-inner"
                >
                  <div className="as-store__search-row">
                    <div className="as-store__search as-store__search-shell">
                      <FaSearch className="as-store__search-icon" />
                      <input
                        type="search"
                        enterKeyHint="search"
                        autoComplete="off"
                        value={searchQuery}
                        onChange={(event) => setSearchQuery(event.target.value)}
                        placeholder={t.tapp.searchApps}
                        className="as-store__search-input"
                      />
                      {searchQuery && (
                        <button
                          type="button"
                          className="as-store__search-clear"
                          onClick={() => setSearchQuery('')}
                          title={t.tapp.clearSearch}
                          aria-label={t.tapp.clearSearch}
                        >
                          <FaTimesCircle className="h-3.5 w-3.5" />
                        </button>
                      )}
                    </div>
                    {isAdmin && (
                      <button
                        type="button"
                        className="as-store__tool"
                        onClick={openStoreConfiguration}
                        title={t.tapp.storeConfiguration}
                      >
                        <FaCog className="h-4 w-4" />
                      </button>
                    )}
                    <button
                      type="button"
                      className="as-store__tool"
                      onClick={() => loadRemoteApps(true)}
                      disabled={loading}
                      title={t.tapp.refreshStore}
                    >
                      {loading ? (
                        <Spinner size="sm" color="current" />
                      ) : (
                        <FaSync className="h-4 w-4" />
                      )}
                    </button>
                  </div>
                  <div
                    className="as-store__cats"
                    role="group"
                    aria-label={t.tapp.categoryFilter}
                  >
                    <CategoryPill
                      active={selectedCategory === null}
                      label={t.tapp.storeDiscover}
                      onClick={() => selectCategory(null)}
                    />
                    <CategoryPill
                      active={selectedCategory === '__installed__'}
                      label={t.tapp.installed}
                      onClick={() => selectCategory('__installed__')}
                    />
                    {categories.map((category) => (
                      <CategoryPill
                        key={category.id}
                        active={selectedCategory === category.id}
                        label={category.name}
                        onClick={() => selectCategory(category.id)}
                      />
                    ))}
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>

          <div className="as-store__views">
            <AnimatePresence mode="wait" initial={false}>
              {detailApp ? (
                <motion.div
                  key={`detail-${detailApp.id}`}
                  {...viewMotionProps(1)}
                  className="as-store__view"
                >
                  <div
                    ref={attachDetailView}
                    className="as-store__scroll as-detail__scroll"
                  >
                    <div className="as-detail__desktop-back">
                      <button
                        type="button"
                        className="as-store__back as-detail__fluid-control glass glass-liquid"
                        onClick={closeSecondaryView}
                        aria-label={t.tapp.back}
                        title={t.tapp.back}
                      >
                        <FaArrowLeft className="h-3.5 w-3.5" />
                      </button>
                      <DetailHeaderActions app={detailApp} />
                    </div>
                    <AppDetailView
                      app={detailApp}
                      isInstalled={installedIds.has(detailApp.id)}
                      installedVersion={detailTappInfo?.version}
                      canUninstall={detailCanUninstall}
                      installing={installingIds.has(detailApp.id)}
                      installPercent={
                        installProgressById.get(detailApp.id)?.percent ?? null
                      }
                      installPhase={
                        installProgressById.get(detailApp.id)?.phase ?? null
                      }
                      installDetail={
                        installProgressById.get(detailApp.id)?.detail ?? null
                      }
                      updating={updatingIds.has(detailApp.id)}
                      onInstall={() => handleInstall(detailApp)}
                      onUpdate={() => handleUpdate(detailApp)}
                      onLaunch={() => launchApp(detailApp.id)}
                      onUninstall={(anchor) =>
                        handleUninstall(detailApp.id, anchor)
                      }
                    />
                  </div>
                </motion.div>
              ) : showStoreConfiguration ? (
                <motion.div
                  key="store-configuration"
                  {...viewMotionProps(1)}
                  className="as-store__view"
                >
                  <div
                    ref={attachDetailView}
                    className="as-store__scroll as-detail__scroll"
                  >
                    <div className="as-detail__desktop-back">
                      <button
                        type="button"
                        className="as-store__back as-detail__fluid-control glass glass-liquid"
                        onClick={closeSecondaryView}
                        aria-label={t.tapp.back}
                        title={t.tapp.back}
                      >
                        <FaArrowLeft className="h-3.5 w-3.5" />
                      </button>
                    </div>
                    <StoreConfigurationView
                      sources={sources}
                      onToggle={handleToggleSource}
                      onRemove={handleRemoveSource}
                      onAdd={handleAddSource}
                      onUpdate={handleUpdateSource}
                      onRefresh={() => loadRemoteApps(true)}
                      refreshing={loading}
                      isAdmin={isAdmin}
                    />
                  </div>
                </motion.div>
              ) : showAllAppsPage ? (
                <motion.div
                  key="all-apps"
                  {...viewMotionProps(1)}
                  className="as-store__view"
                >
                  <div
                    ref={attachDetailView}
                    className="as-store__scroll as-detail__scroll"
                  >
                    <div className="as-detail__desktop-back">
                      <button
                        type="button"
                        className="as-store__back as-detail__fluid-control glass glass-liquid"
                        onClick={closeSecondaryView}
                        aria-label={t.tapp.back}
                        title={t.tapp.back}
                      >
                        <FaArrowLeft className="h-3.5 w-3.5" />
                      </button>
                    </div>
                    <div className="as-store__scroll-pad as-store__all-apps-page">
                      <header className="as-store__page-head">
                        <h2 className="as-store__page-title">{t.tapp.allApps}</h2>
                      </header>
                      <section className="as-store__section">
                        <div className="as-store__section-head">
                          <div
                            className="as-store__sort-control"
                            role="group"
                            aria-label={t.tapp.storeSortOrder}
                          >
                            <button
                              type="button"
                              className="as-store__sort-option"
                              data-active={
                                categorySortOrder === 'name' ? 'true' : 'false'
                              }
                              aria-pressed={categorySortOrder === 'name'}
                              onClick={() => setCategorySortOrder('name')}
                            >
                              {t.tapp.storeSortByName}
                            </button>
                            <button
                              type="button"
                              className="as-store__sort-option"
                              data-active={
                                categorySortOrder === 'date' ? 'true' : 'false'
                              }
                              aria-pressed={categorySortOrder === 'date'}
                              onClick={() => setCategorySortOrder('date')}
                            >
                              {t.tapp.storeSortByDate}
                            </button>
                            <button
                              type="button"
                              className="as-store__sort-option"
                              data-active={
                                categorySortOrder === 'downloads'
                                  ? 'true'
                                  : 'false'
                              }
                              aria-pressed={categorySortOrder === 'downloads'}
                              onClick={() => setCategorySortOrder('downloads')}
                            >
                              {t.tapp.storeSortByDownloads}
                            </button>
                          </div>
                        </div>
                        <div
                          key={categorySortOrder}
                          className="as-store__list"
                        >
                          {allAppsCatalogSorted.map((app, index) =>
                            renderAppCard(app, index),
                          )}
                        </div>
                      </section>
                    </div>
                  </div>
                </motion.div>
              ) : (
                <motion.div
                  key="list"
                  ref={attachListView}
                  {...viewMotionProps(-1)}
                  className="as-store__view as-store__scroll"
                >
                  <div
                    className={`as-store__scroll-pad${
                      isDiscoverView ? ' as-store__scroll-pad--discover' : ''
                    }`}
                  >
                    <header className="as-store__page-head">
                      <h2 key={sectionTitle} className="as-store__page-title">
                        {sectionTitle}
                      </h2>
                    </header>
                    <div
                      key={catalogMotionKey}
                      className="as-store__catalog-body"
                    >
                      <StoreCatalogView
                        loading={loading}
                        error={error}
                        remoteEmpty={remoteApps.length === 0}
                        filteredApps={filteredApps}
                        featuredApps={featuredApps}
                        latestApps={latestApps}
                        selectedCategory={selectedCategory}
                        isDiscoverView={isDiscoverView}
                        sectionTitle={sectionTitle}
                        installedSortOrder={installedSortOrder}
                        categorySortOrder={categorySortOrder}
                        setInstalledSortOrder={setInstalledSortOrder}
                        setCategorySortOrder={setCategorySortOrder}
                        sortedAvailableUpdates={sortedAvailableUpdates}
                        installedCurrentApps={installedCurrentApps}
                        installedTapps={installedTapps}
                        onRetry={() => loadRemoteApps(true)}
                        onOpenDetail={openDetail}
                        onSeeAllApps={openAllAppsPage}
                        renderAppCard={renderAppCard}
                      />
                    </div>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </main>
      </div>

      <UninstallConfirmDialog
        isOpen={showUninstallDialog}
        appName={uninstallTargetName}
        anchorEl={uninstallAnchor}
        onCancel={cancelUninstall}
        onConfirm={handleConfirmUninstall}
      />
    </div>
  )
}

export default TappStore
