import type { RSSHubConfig, RSSHubQueryParams } from '../../../types/brew'
import {
  LuActivity as Activity,
  LuAlertCircle as AlertCircle,
  LuBarChart3 as BarChart3,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuEdit3 as Edit3,
  LuExternalLink as ExternalLink,
  LuFileText as FileText,
  LuGlobe as Globe,
  LuInfo as Info,
  LuKey as Key,
  LuMessageCircle as MessageCircle,
  LuMonitor as Monitor,
  LuNewspaper as Newspaper,
  LuPackage as Package,
  LuPalette as Palette,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuSearch as Search,
  LuServer as Server,
  LuSettings as Settings,
  LuShoppingCart as ShoppingCart,
  LuTrash2 as Trash2,
  LuVideo as Video,
  LuWrench as Wrench,
  LuX as X,
  LuZap as Zap,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { API_URL } from '../../../config'
import { useI18n } from '../../../contexts/I18nContext'

import { getCSRFHeaderName, getCSRFToken } from '../../../utils/csrf'
import { Spinner } from '../../Spinner'
import '../ui/brew.css'

const TRANSITION_NORMAL = { duration: 0.15 } as const
const TRANSITION_SLOW = { duration: 0.2 } as const

const API_BASE = `${API_URL}/api/brew`

interface RsshubInstance {
  id: number
  user_id: number | null
  name: string
  url: string
  has_access_key: boolean
  priority: number
  enabled: boolean
  health_status: 'healthy' | 'degraded' | 'unhealthy' | 'unknown'
  last_health_check: number | null
  last_response_time_ms: number | null
  consecutive_failures: number
  success_rate: number
  created_at: number
}

const HEALTH_STATUS_CONFIG = {
  healthy: {
    labelKey: 'rsshubHealthy' as const,
    bgColor: 'brew-rsshub__status is-ok',
    icon: Check,
  },
  degraded: {
    labelKey: 'rsshubDegraded' as const,
    bgColor: 'brew-rsshub__status is-warn',
    icon: AlertCircle,
  },
  unhealthy: {
    labelKey: 'rsshubUnhealthy' as const,
    bgColor: 'brew-rsshub__status is-bad',
    icon: X,
  },
  unknown: {
    labelKey: 'rsshubUnknown' as const,
    bgColor: 'brew-rsshub__status',
    icon: Activity,
  },
}

function RSSHubIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z" />
    </svg>
  )
}

const COMMON_QUERY_PARAMS_KEYS = [
  {
    key: 'limit',
    labelKey: 'rsshubParamLimit' as const,
    placeholder: '10',
    type: 'number',
    descKey: 'rsshubParamLimitDesc' as const,
  },
  {
    key: 'mode',
    labelKey: 'rsshubParamMode' as const,
    placeholder: 'fulltext',
    type: 'select',
    options: [
      { value: '', labelKey: 'rsshubDefault' as const },
      { value: 'fulltext', labelKey: 'rsshubFulltext' as const },
    ],
    descKey: 'rsshubParamModeDesc' as const,
  },
  {
    key: 'filter',
    labelKey: 'rsshubParamFilter' as const,
    placeholder: '',
    type: 'text',
    descKey: 'rsshubParamFilterDesc' as const,
  },
  {
    key: 'filter_title',
    labelKey: 'rsshubParamFilterTitle' as const,
    placeholder: '',
    type: 'text',
    descKey: 'rsshubParamFilterTitleDesc' as const,
  },
  {
    key: 'filterout',
    labelKey: 'rsshubParamFilterout' as const,
    placeholder: '',
    type: 'text',
    descKey: 'rsshubParamFilteroutDesc' as const,
  },
  {
    key: 'filterout_title',
    labelKey: 'rsshubParamFilteroutTitle' as const,
    placeholder: '',
    type: 'text',
    descKey: 'rsshubParamFilteroutTitleDesc' as const,
  },
  {
    key: 'filter_time',
    labelKey: 'rsshubParamFilterTime' as const,
    placeholder: '',
    type: 'number',
    descKey: 'rsshubParamFilterTimeDesc' as const,
  },
  {
    key: 'format',
    labelKey: 'rsshubParamFormat' as const,
    placeholder: 'rss',
    type: 'select',
    options: [
      { value: '', labelKey: 'rsshubDefaultRss' as const },
      { value: 'atom', labelKey: 'rsshubAtom' as const },
      { value: 'json', labelKey: 'rsshubJson' as const },
    ],
    descKey: 'rsshubParamFormatDesc' as const,
  },
]

const ROUTE_CATEGORIES_KEYS = [
  {
    id: 'social',
    nameKey: 'rsshubCategorySocial' as const,
    icon: <MessageCircle size={12} />,
  },
  {
    id: 'video',
    nameKey: 'rsshubCategoryVideo' as const,
    icon: <Video size={12} />,
  },
  {
    id: 'news',
    nameKey: 'rsshubCategoryNews' as const,
    icon: <Newspaper size={12} />,
  },
  {
    id: 'blog',
    nameKey: 'rsshubCategoryBlog' as const,
    icon: <FileText size={12} />,
  },
  {
    id: 'programming',
    nameKey: 'rsshubCategoryProgramming' as const,
    icon: <Monitor size={12} />,
  },
  {
    id: 'design',
    nameKey: 'rsshubCategoryDesign' as const,
    icon: <Palette size={12} />,
  },
  {
    id: 'shopping',
    nameKey: 'rsshubCategoryShopping' as const,
    icon: <ShoppingCart size={12} />,
  },
  {
    id: 'other',
    nameKey: 'rsshubCategoryOther' as const,
    icon: <Package size={12} />,
  },
]

type RouteConfigRequirement = 'none' | 'server' | 'optional'

interface RouteTemplate {
  path: string
  requiresConfig?: RouteConfigRequirement
  configNote?: string
}

function rsshubRouteName(
  names: Record<string, string>,
  path: string,
): string {
  return names[path] || path
}

const POPULAR_ROUTES: { category: string; routes: RouteTemplate[] }[] = [
  {
    category: 'social',
    routes: [
      {
        path: '/bsky/profile/:handle',
      },
      {
        path: '/douban/people/:id/status',
      },
      { path: '/douban/group/:groupid' },
      { path: '/douban/movie/playing' },
      { path: '/douban/explore' },
      {
        path: '/telegram/channel/:id',
      },
      {
        path: '/weibo/oasis/user/:userid',
      },
    ],
  },
  {
    category: 'video',
    routes: [
      {
        path: '/bilibili/user/video/:uid',
      },
      {
        path: '/bilibili/ranking/:rid?',
      },
      { path: '/bilibili/popular/all' },
      { path: '/bilibili/weekly' },
      { path: '/bilibili/precious' },
      { path: '/bilibili/hot-search' },
      {
        path: '/bilibili/bangumi/media/:mediaid',
      },
      {
        path: '/bilibili/user/article/:uid',
      },
      { path: '/bilibili/audio/:id' },
      {
        path: '/acfun/user/video/:uid',
      },
    ],
  },
  {
    category: 'news',
    routes: [
      { path: '/sspai/index' },
      { path: '/sspai/matrix' },
      {
        path: '/sspai/author/:id',
      },
      { path: '/sspai/tag/:keyword' },
      { path: '/sspai/topic/:id' },
      { path: '/36kr/hot-list' },
      { path: '/36kr/newsflashes' },
      { path: '/thepaper/featured' },
      { path: '/zhihu/daily' },
      { path: '/cls/telegraph' },
    ],
  },
  {
    category: 'programming',
    routes: [
      {
        path: '/github/repos/:user',
      },
      {
        path: '/github/issue/:user/:repo',
      },
      {
        path: '/github/pull/:user/:repo',
      },
      {
        path: '/github/wiki/:user/:repo/:page?',
      },
      {
        path: '/github/topics/:name',
      },
      { path: '/hellogithub/home' },
      { path: '/hellogithub/volume' },
      {
        path: '/huggingface/daily-papers',
      },
      { path: '/anthropic/news' },
      { path: '/anthropic/research' },
      { path: '/web/articles' },
      { path: '/web/blog' },
      { path: '/hackernews/best' },
    ],
  },
  {
    category: 'blog',
    routes: [
      {
        path: '/rsshub/routes/:lang?',
      },
      { path: '/zhubai/:id' },
      { path: '/substack/:id' },
      { path: '/xlog/:handle' },
      { path: '/wordpress/:domain' },
      {
        path: '/zhiy/letters/:author',
      },
    ],
  },
  {
    category: 'design',
    routes: [
      {
        path: '/dribbble/popular/:timeframe?',
      },
      {
        path: '/dribbble/user/:name',
      },
      {
        path: '/zcool/discover/:type?',
      },
      { path: '/zcool/user/:uid' },
      { path: '/topys' },
    ],
  },
  {
    category: 'shopping',
    routes: [
      {
        path: '/smzdm/keyword/:keyword',
      },
      {
        path: '/smzdm/ranking/:rank_type/:rank_id',
      },
    ],
  },
  {
    category: 'other',
    routes: [
      { path: '/douban/book/latest' },
      { path: '/bangumi/calendar/today' },
      {
        path: '/steam/search/:params',
      },
      {
        path: '/earthquake/:region?',
      },
    ],
  },
]

interface RSSHubConfigProps {
  initialConfig?: RSSHubConfig
  onConfigChange: (config: RSSHubConfig, fullUrl: string) => void
  isEditMode?: boolean
  disabled?: boolean
}

export default function RSSHubConfigComponent({
  initialConfig,
  onConfigChange,
  isEditMode: _isEditMode = false,
  disabled = false,
}: RSSHubConfigProps) {
  const { t, locale, format } = useI18n()

  const [instances, setInstances] = useState<RsshubInstance[]>([])
  const [loadingInstances, setLoadingInstances] = useState(true)
  const [instanceError, setInstanceError] = useState<string | null>(null)

  const [selectedInstanceId, setSelectedInstanceId] = useState<number | null>(
    null,
  )

  const [routePath, setRoutePath] = useState(initialConfig?.routePath || '')
  const [routeParams, setRouteParams] = useState<Record<string, string>>(
    initialConfig?.routeParams ?? {},
  )

  const [queryParams, setQueryParams] = useState<RSSHubQueryParams>(
    initialConfig?.queryParams ?? {},
  )
  const [showAdvancedOptions, setShowAdvancedOptions] = useState(
    Object.keys(initialConfig?.queryParams ?? {}).length > 0,
  )
  const [customQueryKey, setCustomQueryKey] = useState('')
  const [customQueryValue, setCustomQueryValue] = useState('')

  const [currentRouteConfig, setCurrentRouteConfig] = useState<{
    requiresConfig?: RouteConfigRequirement
    configNote?: string
  } | null>(null)

  const [showInstancePanel, setShowInstancePanel] = useState(false)
  const [showRouteExplorer, setShowRouteExplorer] = useState(false)
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null)
  const [searchQuery, setSearchQuery] = useState('')

  const [showAddForm, setShowAddForm] = useState(false)
  const [newName, setNewName] = useState('')
  const [newUrl, setNewUrl] = useState('')
  const [newAccessKey, setNewAccessKey] = useState('')
  const [newPriority, setNewPriority] = useState(100)
  const [adding, setAdding] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)
  const [editName, setEditName] = useState('')
  const [editUrl, setEditUrl] = useState('')
  const [editAccessKey, setEditAccessKey] = useState('')
  const [editPriority, setEditPriority] = useState(100)
  const [checkingHealth, setCheckingHealth] = useState<number | null>(null)
  const [checkingAllHealth, setCheckingAllHealth] = useState(false)

  const [testing, setTesting] = useState(false)
  const [testResult, setTestResult] = useState<{
    success: boolean
    message: string
  } | null>(null)

  const currentInstance = useMemo(
    () =>
      instances.find((i) => i.id === selectedInstanceId) ??
      instances.find((i) => i.enabled) ??
      instances[0],
    [instances, selectedInstanceId],
  )

  const getAuthHeaders = useCallback(async () => {
    const cookies = document.cookie.split(';').reduce(
      (acc, cookie) => {
        const [key, value] = cookie.trim().split('=')
        acc[key] = value
        return acc
      },
      {} as Record<string, string>,
    )
    const authToken = cookies.auth_token

    // CSRF 从服务端取最新。
    const csrfToken = await getCSRFToken(true)

    return {
      'Content-Type': 'application/json',
      ...(authToken ? { Authorization: `Bearer ${authToken}` } : {}),
      ...(csrfToken ? { [getCSRFHeaderName()]: csrfToken } : {}),
    }
  }, [])

  const loadInstances = useCallback(async () => {
    try {
      setLoadingInstances(true)
      setInstanceError(null)
      const response = await fetch(`${API_BASE}/rsshub/instances`, {
        headers: await getAuthHeaders(),
      })
      const data = await response.json()
      if (data.success) {
        const loadedInstances = data.instances || []
        setInstances(loadedInstances)
        if (initialConfig?.instanceUrl && !selectedInstanceId) {
          const matched = loadedInstances.find(
            (i: RsshubInstance) => i.url === initialConfig.instanceUrl,
          )
          if (matched) {
            setSelectedInstanceId(matched.id)
          } else if (loadedInstances.length > 0) {
            const enabled = loadedInstances.find(
              (i: RsshubInstance) => i.enabled,
            )
            setSelectedInstanceId(enabled?.id || loadedInstances[0].id)
          }
        } else if (!selectedInstanceId && loadedInstances.length > 0) {
          const enabled = loadedInstances.find((i: RsshubInstance) => i.enabled)
          setSelectedInstanceId(enabled?.id || loadedInstances[0].id)
        }
      } else {
        setInstanceError(data.error || t.brew.errorLoadFailed)
      }
    } catch {
      setInstanceError(t.brew.errorNetworkRetry)
    } finally {
      setLoadingInstances(false)
    }
  }, [getAuthHeaders, initialConfig?.instanceUrl, selectedInstanceId])

  useEffect(() => {
    loadInstances()
  }, [])

  const handleAddInstance = async () => {
    if (!newName.trim() || !newUrl.trim()) return
    try {
      setAdding(true)
      const response = await fetch(`${API_BASE}/rsshub/instances`, {
        method: 'POST',
        headers: await getAuthHeaders(),
        body: JSON.stringify({
          name: newName.trim(),
          url: newUrl.trim().replaceAll(/\/$/g, ''),
          access_key: newAccessKey.trim() || null,
          priority: newPriority,
        }),
      })
      const data = await response.json()
      if (data.success) {
        setInstances((prev) => [...prev, data.instance])
        setSelectedInstanceId(data.instance.id)
        setShowAddForm(false)
        setNewName('')
        setNewUrl('')
        setNewAccessKey('')
        setNewPriority(100)
      } else {
        setInstanceError(data.error || t.brew.errorAddFailed)
      }
    } catch {
      setInstanceError(t.brew.errorNetworkRetry)
    } finally {
      setAdding(false)
    }
  }

  const handleUpdateInstance = async (id: number) => {
    try {
      const response = await fetch(`${API_BASE}/rsshub/instances/${id}`, {
        method: 'PUT',
        headers: await getAuthHeaders(),
        body: JSON.stringify({
          name: editName.trim() || undefined,
          url: editUrl.trim().replaceAll(/\/$/g, '') || undefined,
          access_key: editAccessKey.trim() || undefined,
          priority: editPriority,
        }),
      })
      const data = await response.json()
      if (data.success) {
        setInstances((prev) =>
          prev.map((i) => (i.id === id ? data.instance : i)),
        )
        setEditingId(null)
      } else {
        setInstanceError(data.error || t.brew.errorUpdateFailed)
      }
    } catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
  }

  const handleDeleteInstance = async (id: number) => {
    if (!confirm(t.brew.rsshubConfirmDelete)) return
    try {
      const response = await fetch(`${API_BASE}/rsshub/instances/${id}`, {
        method: 'DELETE',
        headers: await getAuthHeaders(),
      })
      const data = await response.json()
      if (data.success) {
        setInstances((prev) => prev.filter((i) => i.id !== id))
        if (selectedInstanceId === id) {
          const remaining = instances.filter((i) => i.id !== id)
          setSelectedInstanceId(
            remaining.find((i) => i.enabled)?.id || remaining[0]?.id || null,
          )
        }
      } else {
        setInstanceError(data.error || t.brew.errorDeleteFailed)
      }
    } catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
  }

  const handleToggleEnabled = async (instance: RsshubInstance) => {
    try {
      const response = await fetch(
        `${API_BASE}/rsshub/instances/${instance.id}`,
        {
          method: 'PUT',
          headers: await getAuthHeaders(),
          body: JSON.stringify({ enabled: !instance.enabled }),
        },
      )
      const data = await response.json()
      if (data.success) {
        setInstances((prev) =>
          prev.map((i) => (i.id === instance.id ? data.instance : i)),
        )
      }
    } catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
  }

  const handleHealthCheck = async (id: number) => {
    try {
      setCheckingHealth(id)
      const response = await fetch(
        `${API_BASE}/rsshub/instances/${id}/health-check`,
        { method: 'POST', headers: await getAuthHeaders() },
      )
      const data = await response.json()
      if (data.success) {
        await loadInstances()
      }
    } catch {
      setInstanceError(t.brew.errorHealthCheckFailed)
    } finally {
      setCheckingHealth(null)
    }
  }

  const handleHealthCheckAll = async () => {
    try {
      setCheckingAllHealth(true)
      const response = await fetch(`${API_BASE}/rsshub/health-check-all`, {
        method: 'POST',
        headers: await getAuthHeaders(),
      })
      const data = await response.json()
      if (data.success) {
        await loadInstances()
      }
    } catch {
      setInstanceError(t.brew.errorHealthCheckFailed)
    } finally {
      setCheckingAllHealth(false)
    }
  }

  const handleResetInstance = async (id: number) => {
    try {
      const response = await fetch(`${API_BASE}/rsshub/instances/${id}/reset`, {
        method: 'POST',
        headers: await getAuthHeaders(),
      })
      const data = await response.json()
      if (data.success) {
        await loadInstances()
      }
    } catch {
      setInstanceError(t.brew.errorResetFailed)
    }
  }

  const startEditInstance = (instance: RsshubInstance) => {
    setEditingId(instance.id)
    setEditName(instance.name)
    setEditUrl(instance.url)
    setEditAccessKey('')
    setEditPriority(instance.priority)
  }

  const formatTime = (timestamp: number | null) => {
    if (!timestamp) return t.brew.rsshubNever
    const date = new Date(timestamp)
    return date.toLocaleString(locale, {
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  const fullUrl = useMemo(() => {
    if (!routePath || !currentInstance) return ''
    const baseUrl = currentInstance.url
    let path = routePath
    Object.entries(routeParams).forEach(([key, value]) => {
      if (value) {
        path = path.replace(`:${key}`, value).replace(`:${key}?`, value)
      }
    })
    path = path.replaceAll(/\/:[^/]+\?/g, '')

    const queryParts: string[] = []

    Object.entries(queryParams).forEach(([key, value]) => {
      if (value !== undefined && value !== '' && value !== null) {
        queryParts.push(
          `${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`,
        )
      }
    })

    const queryString = queryParts.length > 0 ? `?${queryParts.join('&')}` : ''
    return `${baseUrl}${path}${queryString}`
  }, [currentInstance, routePath, routeParams, queryParams])

  const extractedParams = useMemo(() => {
    const matches = routePath.match(/:([^/]+)/g) ?? []
    return matches.map((m) => ({
      name: m.slice(1).replace('?', ''),
      required: !m.endsWith('?'),
    }))
  }, [routePath])

  useEffect(() => {
    if (routePath && currentInstance) {
      const config: RSSHubConfig = {
        instanceUrl: currentInstance.url,
        routePath,
        routeParams,
        queryParams:
          Object.keys(queryParams).length > 0 ? queryParams : undefined,
      }
      onConfigChange(config, fullUrl)
    }
  }, [
    currentInstance,
    routePath,
    routeParams,
    queryParams,
    fullUrl,
    onConfigChange,
  ])

  const handleTestConnection = async () => {
    if (!fullUrl) return
    setTesting(true)
    setTestResult(null)
    try {
      await fetch(fullUrl, {
        method: 'HEAD',
        mode: 'no-cors',
      })
      setTestResult({ success: true, message: t.brew.rsshubConnectionOk })
    } catch {
      setTestResult({ success: false, message: t.brew.rsshubConnectionFailed })
    } finally {
      setTesting(false)
    }
  }

  const handleSelectRoute = (route: RouteTemplate) => {
    setRoutePath(route.path)
    setRouteParams({})
    setCurrentRouteConfig(
      route.requiresConfig
        ? {
            requiresConfig: route.requiresConfig,
            configNote: route.configNote,
          }
        : null,
    )
    setShowRouteExplorer(false)
  }

  const filteredRoutes = useMemo(() => {
    if (!searchQuery && !selectedCategory) return POPULAR_ROUTES

    return POPULAR_ROUTES.map((category) => ({
      ...category,
      routes: category.routes.filter((route) => {
        const matchesCategory =
          !selectedCategory || category.category === selectedCategory
        const label = rsshubRouteName(
          t.brew.rsshubRouteNames as Record<string, string>,
          route.path,
        )
        const matchesSearch =
          !searchQuery ||
          label.toLowerCase().includes(searchQuery.toLowerCase()) ||
          route.path.toLowerCase().includes(searchQuery.toLowerCase())
        return matchesCategory && matchesSearch
      }),
    })).filter((category) => category.routes.length > 0)
  }, [searchQuery, selectedCategory, t.brew.rsshubRouteNames])

  return (
    <div className="brew-skin brew-rsshub">
      <AnimatePresence>
        {instanceError && (
          <motion.div
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className="p-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-700/50 rounded-lg flex items-center justify-between"
          >
            <div className="flex items-center gap-2 text-xs text-red-600 dark:text-red-400">
              <AlertCircle className="w-3 h-3" />
              {instanceError}
            </div>
            <button
              type="button"
              onClick={() => setInstanceError(null)}
              className="text-red-400 hover:text-red-600"
              title={t.common.close}
              aria-label={t.brew.rsshubCloseError}
            >
              <X className="w-3 h-3" />
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="border border-gray-200/80 dark:border-neutral-700/80 rounded-xl overflow-hidden">
        <button
          type="button"
          onClick={() => !disabled && setShowInstancePanel(!showInstancePanel)}
          disabled={disabled}
          className="w-full px-3 py-2.5 bg-gray-50/80 dark:bg-neutral-800/50 flex items-center justify-between text-sm text-left focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all disabled:opacity-50"
        >
          <div className="flex items-center gap-2 min-w-0">
            <RSSHubIcon className="w-4 h-4 text-orange-500 shrink-0" />
            <div className="min-w-0">
              {loadingInstances ? (
                <div className="flex items-center gap-2">
                  <Spinner size="xs" />
                  <span className="text-gray-400">{t.brew.loading}</span>
                </div>
              ) : currentInstance ? (
                <>
                  <div className="flex items-center gap-1.5">
                    <span className="font-medium text-gray-800 dark:text-gray-100 truncate">
                      {currentInstance.name}
                    </span>
                    {currentInstance.has_access_key && (
                      <span title={t.brew.rsshubHasAccessKey}>
                        <Key className="w-3 h-3 text-amber-500" />
                      </span>
                    )}
                    <span
                      className={
                        HEALTH_STATUS_CONFIG[currentInstance.health_status]
                          .bgColor
                      }
                    >
                      {
                        t.brew[
                          HEALTH_STATUS_CONFIG[currentInstance.health_status]
                            .labelKey
                        ]
                      }
                    </span>
                  </div>
                  <div className="text-xs text-gray-400 truncate">
                    {currentInstance.url}
                  </div>
                </>
              ) : (
                <span className="text-gray-400">{t.brew.rsshubNoInstance}</span>
              )}
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {instances.length > 0 && (
              <span className="text-xs text-gray-400">
                {format(t.brew.rsshubInstanceCount, {
                  count: instances.length,
                })}
              </span>
            )}
            <ChevronDown
              className={`w-4 h-4 text-gray-400 transition-transform ${showInstancePanel ? 'rotate-180' : ''}`}
            />
          </div>
        </button>

        <AnimatePresence>
          {showInstancePanel && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={TRANSITION_SLOW}
              className="overflow-hidden border-t border-gray-200/80 dark:border-neutral-700/80"
            >
              <div className="p-3 space-y-3 bg-white dark:bg-neutral-800">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={handleHealthCheckAll}
                      disabled={checkingAllHealth || loadingInstances}
                      className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
                    >
                      {checkingAllHealth ? (
                        <Spinner size="xs" color="current" />
                      ) : (
                        <RefreshCw className="w-3 h-3" />
                      )}
                      {t.brew.rsshubCheckAll}
                    </button>
                    <button
                      type="button"
                      onClick={() => setShowAddForm(true)}
                      className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700"
                    >
                      <Plus className="w-3 h-3" />
                      {t.brew.rsshubAddInstance}
                    </button>
                  </div>
                </div>

                <AnimatePresence>
                  {showAddForm && (
                    <motion.div
                      initial={{ opacity: 0, height: 0 }}
                      animate={{ opacity: 1, height: 'auto' }}
                      exit={{ opacity: 0, height: 0 }}
                      className="overflow-hidden"
                    >
                      <div className="p-3 bg-orange-50 dark:bg-orange-900/20 border border-orange-200/50 dark:border-orange-700/50 rounded-lg space-y-2">
                        <h4 className="font-medium text-sm text-gray-800 dark:text-gray-100 flex items-center gap-2">
                          <Plus className="w-3.5 h-3.5 text-orange-500" />
                          {t.brew.rsshubAddNewInstance}
                        </h4>
                        <input
                          type="text"
                          value={newName}
                          onChange={(e) => setNewName(e.target.value)}
                          placeholder={t.brew.rsshubInstanceName}
                          className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                        />
                        <input
                          type="url"
                          value={newUrl}
                          onChange={(e) => setNewUrl(e.target.value)}
                          placeholder={t.brew.rsshubInstanceUrl}
                          className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 font-mono"
                        />
                        <input
                          type="password"
                          value={newAccessKey}
                          onChange={(e) => setNewAccessKey(e.target.value)}
                          placeholder={t.brew.rsshubAccessKey}
                          className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                        />
                        <div className="flex items-center gap-2">
                          <label
                            className="text-[11px] text-gray-500"
                            htmlFor="new-priority-input"
                          >
                            {t.brew.rsshubPriority}:
                          </label>
                          <input
                            id="new-priority-input"
                            type="number"
                            value={newPriority}
                            onChange={(e) =>
                              setNewPriority(Number(e.target.value))
                            }
                            min={0}
                            max={999}
                            title={t.brew.rsshubPriority}
                            className="w-16 px-2 py-1 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                          />
                          <span className="text-[11px] text-gray-400">
                            {t.brew.rsshubPriorityHint}
                          </span>
                        </div>
                        <div className="flex gap-2 pt-1">
                          <button
                            type="button"
                            onClick={() => {
                              setShowAddForm(false)
                              setNewName('')
                              setNewUrl('')
                              setNewAccessKey('')
                              setNewPriority(100)
                            }}
                            className="flex-1 px-2.5 py-1.5 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg"
                          >
                            {t.brew.cancel}
                          </button>
                          <button
                            type="button"
                            onClick={handleAddInstance}
                            disabled={
                              adding || !newName.trim() || !newUrl.trim()
                            }
                            className="flex-1 px-2.5 py-1.5 text-xs bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900 rounded-lg hover:bg-gray-700 dark:hover:bg-gray-200 disabled:opacity-50 flex items-center justify-center gap-1"
                          >
                            {adding && (
                              <Spinner size="xs" color="current" />
                            )}
                            {t.brew.add}
                          </button>
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>

                {loadingInstances ? (
                  <div className="flex items-center justify-center py-6">
                    <Spinner size="sm" className="text-orange-500" />
                  </div>
                ) : instances.length === 0 ? (
                  <div className="text-center py-6 text-gray-400">
                    <Server className="w-8 h-8 mx-auto mb-2 opacity-50" />
                    <p className="text-xs">{t.brew.rsshubNoInstances}</p>
                    <p className="text-[10px] mt-0.5">
                      {t.brew.rsshubClickToAdd}
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2 max-h-64 overflow-y-auto">
                    {instances.map((instance) => {
                      const status =
                        HEALTH_STATUS_CONFIG[instance.health_status]
                      const StatusIcon = status.icon
                      const isEditing = editingId === instance.id
                      const isSelected = selectedInstanceId === instance.id

                      return (
                        <div
                          key={instance.id}
                          className={`p-2.5 rounded-lg border transition-all cursor-pointer ${
                            isSelected
                              ? 'bg-orange-50 dark:bg-orange-900/20 border-orange-300 dark:border-orange-700'
                              : instance.enabled
                                ? 'bg-gray-50 dark:bg-neutral-900 border-gray-200 dark:border-neutral-700 hover:border-orange-300 dark:hover:border-orange-700'
                                : 'bg-gray-100 dark:bg-neutral-900/50 border-gray-100 dark:border-neutral-800 opacity-60'
                          }`}
                          onClick={() =>
                            !isEditing && setSelectedInstanceId(instance.id)
                          }
                        >
                          {isEditing ? (
                            <div
                              className="space-y-2"
                              onClick={(e) => e.stopPropagation()}
                            >
                              <input
                                type="text"
                                value={editName}
                                onChange={(e) => setEditName(e.target.value)}
                                placeholder={t.brew.rsshubInstanceName}
                                className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                              />
                              <input
                                type="url"
                                value={editUrl}
                                onChange={(e) => setEditUrl(e.target.value)}
                                placeholder={t.brew.rsshubInstanceUrlShort}
                                className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 font-mono"
                              />
                              <input
                                type="password"
                                value={editAccessKey}
                                onChange={(e) =>
                                  setEditAccessKey(e.target.value)
                                }
                                placeholder={t.brew.rsshubAccessKeyKeep}
                                className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                              />
                              <div className="flex items-center gap-2">
                                <label
                                  className="text-[11px] text-gray-500"
                                  htmlFor="edit-priority-input"
                                >
                                  {t.brew.rsshubPriority}:
                                </label>
                                <input
                                  id="edit-priority-input"
                                  type="number"
                                  value={editPriority}
                                  onChange={(e) =>
                                    setEditPriority(Number(e.target.value))
                                  }
                                  min={0}
                                  max={999}
                                  title={t.brew.rsshubPriority}
                                  className="w-16 px-2 py-1 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                                />
                              </div>
                              <div className="flex gap-2 pt-1">
                                <button
                                  type="button"
                                  onClick={() => setEditingId(null)}
                                  className="flex-1 px-2.5 py-1.5 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg"
                                >
                                  {t.brew.cancel}
                                </button>
                                <button
                                  type="button"
                                  onClick={() =>
                                    handleUpdateInstance(instance.id)
                                  }
                                  className="flex-1 px-2.5 py-1.5 text-xs bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900 rounded-lg hover:bg-gray-700 dark:hover:bg-gray-200"
                                >
                                  {t.brew.save}
                                </button>
                              </div>
                            </div>
                          ) : (
                            <>
                              <div className="flex items-start justify-between mb-1.5">
                                <div className="flex items-center gap-2 min-w-0">
                                  <div
                                    className={`p-1 rounded ${status.bgColor}`}
                                    title={t.brew[status.labelKey]}
                                  >
                                    <StatusIcon
                                      className="w-3 h-3"
                                    />
                                  </div>
                                  <div className="min-w-0">
                                    <div className="flex items-center gap-1.5">
                                      <h4 className="text-sm font-medium text-gray-800 dark:text-gray-100 truncate">
                                        {instance.name}
                                      </h4>
                                      {instance.has_access_key && (
                                        <span title={t.brew.rsshubHasAccessKey}>
                                          <Key className="w-3 h-3 text-gray-500" />
                                        </span>
                                      )}
                                    </div>
                                    <div className="text-[11px] text-gray-400 font-mono truncate">
                                      {instance.url}
                                    </div>
                                  </div>
                                </div>
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    handleToggleEnabled(instance)
                                  }}
                                  title={
                                    instance.enabled
                                      ? t.brew.rsshubDisable
                                      : t.brew.rsshubEnable
                                  }
                                  className={`relative w-8 h-4 rounded-full transition-colors shrink-0 ${
                                    instance.enabled
                                      ? 'bg-gray-700 dark:bg-gray-300'
                                      : 'bg-gray-300 dark:bg-neutral-600'
                                  }`}
                                >
                                  <span
                                    className={`absolute top-0.5 left-0.5 w-3 h-3 bg-white rounded-full shadow transition-transform ${
                                      instance.enabled
                                        ? 'translate-x-4'
                                        : 'translate-x-0'
                                    }`}
                                  />
                                </button>
                              </div>

                              <div className="grid grid-cols-4 gap-1.5 mb-1.5">
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">
                                    {t.brew.rsshubPriority}
                                  </div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                    {instance.priority}
                                  </div>
                                </div>
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">
                                    {t.brew.rsshubSuccessRate}
                                  </div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                    {instance.success_rate.toFixed(0)}%
                                  </div>
                                </div>
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">
                                    {t.brew.rsshubResponse}
                                  </div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                    {instance.last_response_time_ms
                                      ? `${instance.last_response_time_ms}ms`
                                      : '-'}
                                  </div>
                                </div>
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">
                                    {t.brew.rsshubLastCheck}
                                  </div>
                                  <div
                                    className="text-xs font-medium text-gray-700 dark:text-gray-300 truncate"
                                    title={formatTime(
                                      instance.last_health_check,
                                    )}
                                  >
                                    {formatTime(instance.last_health_check)}
                                  </div>
                                </div>
                              </div>

                              <div
                                className="flex items-center gap-1"
                                onClick={(e) => e.stopPropagation()}
                              >
                                <button
                                  type="button"
                                  onClick={() => handleHealthCheck(instance.id)}
                                  disabled={checkingHealth === instance.id}
                                  className="flex-1 flex items-center justify-center gap-1 px-1.5 py-1 text-[10px] border border-gray-200 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
                                >
                                  {checkingHealth === instance.id ? (
                                    <Spinner size={10} />
                                  ) : (
                                    <RefreshCw className="w-2.5 h-2.5" />
                                  )}
                                  {t.brew.rsshubCheck}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => startEditInstance(instance)}
                                  className="flex-1 flex items-center justify-center gap-1 px-1.5 py-1 text-[10px] border border-gray-200 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded hover:bg-gray-100 dark:hover:bg-neutral-700"
                                >
                                  <Edit3 className="w-2.5 h-2.5" />
                                  {t.brew.edit}
                                </button>
                                <button
                                  type="button"
                                  onClick={() =>
                                    handleResetInstance(instance.id)
                                  }
                                  className="flex-1 flex items-center justify-center gap-1 px-1.5 py-1 text-[10px] border border-gray-200 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded hover:bg-gray-100 dark:hover:bg-neutral-700"
                                >
                                  <BarChart3 className="w-2.5 h-2.5" />
                                  {t.brew.rsshubReset}
                                </button>
                                <button
                                  type="button"
                                  onClick={() =>
                                    handleDeleteInstance(instance.id)
                                  }
                                  className="p-1 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded"
                                  title={t.brew.delete}
                                >
                                  <Trash2 className="w-3 h-3" />
                                </button>
                              </div>
                            </>
                          )}
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <div>
        <div className="flex items-center justify-between mb-1.5">
          <label className="flex items-center gap-2 text-xs font-medium text-gray-600 dark:text-gray-400">
            <Globe className="w-3.5 h-3.5" />
            {t.brew.rsshubRoutePath} <span className="text-rose-500">*</span>
          </label>
          <button
            type="button"
            onClick={() => setShowRouteExplorer(!showRouteExplorer)}
            disabled={disabled}
            className="text-xs text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 flex items-center gap-1"
          >
            <Zap className="w-3 h-3" />
            {t.brew.rsshubBrowsePopular}
          </button>
        </div>

        <div className="relative">
          <input
            type="text"
            value={routePath}
            onChange={(e) => setRoutePath(e.target.value)}
            placeholder="/bilibili/user/video/:uid"
            disabled={disabled}
            className="w-full px-3 py-2.5 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/80 dark:border-neutral-700/80 rounded-xl text-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all font-mono disabled:opacity-50"
          />
        </div>

        <AnimatePresence>
          {showRouteExplorer && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              className="mt-2 overflow-hidden"
            >
              <div className="p-3 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/50 dark:border-neutral-700/50 rounded-xl space-y-3">
                <div className="flex gap-2">
                  <div className="relative flex-1">
                    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder={t.brew.rsshubSearchRoute}
                      className="w-full pl-8 pr-3 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                    />
                  </div>
                </div>

                <div className="flex flex-wrap gap-1.5">
                  <button
                    type="button"
                    onClick={() => setSelectedCategory(null)}
                    className={`px-2 py-1 text-xs rounded-lg transition-colors ${!selectedCategory ? 'bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900' : 'bg-gray-100 dark:bg-neutral-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-neutral-600'}`}
                  >
                    {t.brew.all}
                  </button>
                  {ROUTE_CATEGORIES_KEYS.map((cat) => (
                    <button
                      key={cat.id}
                      type="button"
                      onClick={() => setSelectedCategory(cat.id)}
                      className={`px-2 py-1 text-xs rounded-lg transition-colors flex items-center gap-1 ${selectedCategory === cat.id ? 'bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900' : 'bg-gray-100 dark:bg-neutral-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-neutral-600'}`}
                    >
                      <span>{cat.icon}</span>
                      {t.brew[cat.nameKey]}
                    </button>
                  ))}
                </div>

                <div className="flex items-center gap-3 text-[10px] text-gray-500 dark:text-gray-400 py-1 border-b border-gray-200/50 dark:border-neutral-700/50">
                  <span className="flex items-center gap-1">
                    <span className="px-1 py-0.5 bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400 rounded">
                      <Settings size={10} />
                    </span>
                    {t.brew.rsshubServerConfig}
                  </span>
                  <span className="flex items-center gap-1">
                    <span className="px-1 py-0.5 bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded">
                      <Wrench size={10} />
                    </span>
                    {t.brew.rsshubOptionalConfig}
                  </span>
                </div>

                <div className="max-h-48 overflow-y-auto space-y-2">
                  {filteredRoutes.map((category) => (
                    <div key={category.category}>
                      <div className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                        {
                          ROUTE_CATEGORIES_KEYS.find(
                            (c) => c.id === category.category,
                          )?.icon
                        }{' '}
                        {(() => {
                          const cat = ROUTE_CATEGORIES_KEYS.find(
                            (c) => c.id === category.category,
                          )
                          return cat ? t.brew[cat.nameKey] : ''
                        })()}
                      </div>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-1">
                        {category.routes.map((route) => (
                          <button
                            key={route.path}
                            type="button"
                            onClick={() => handleSelectRoute(route)}
                            className="flex items-center gap-2 px-2.5 py-2 text-left text-xs rounded-lg bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 hover:border-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/20 transition-colors"
                          >
                            <div className="min-w-0 flex-1">
                              <div className="font-medium text-gray-800 dark:text-gray-100 truncate flex items-center gap-1">
                                {rsshubRouteName(
                                  t.brew.rsshubRouteNames as Record<string, string>,
                                  route.path,
                                )}
                                {route.requiresConfig === 'server' && (
                                  <span
                                    className="px-1 py-0.5 text-[10px] bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400 rounded"
                                    title={t.brew.rsshubServerConfig}
                                  >
                                    <Settings size={10} />
                                  </span>
                                )}
                                {route.requiresConfig === 'optional' && (
                                  <span
                                    className="px-1 py-0.5 text-[10px] bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded"
                                    title={t.brew.rsshubOptionalConfig}
                                  >
                                    <Wrench size={10} />
                                  </span>
                                )}
                              </div>
                              <div className="text-gray-400 truncate font-mono">
                                {route.path}
                              </div>
                            </div>
                          </button>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>

                <div className="pt-2 border-t border-gray-200 dark:border-neutral-700">
                  <a
                    href="https://docs.rsshub.app/routes"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-xs text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 flex items-center gap-1"
                  >
                    <ExternalLink className="w-3 h-3" />
                    {t.brew.rsshubViewFullDocs}
                  </a>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {extractedParams.length > 0 && (
        <div>
          <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1.5">
            {t.brew.rsshubRouteParams}
          </label>
          <div className="space-y-2">
            {extractedParams.map((param) => (
              <div key={param.name} className="flex items-center gap-2">
                <span className="text-xs text-gray-500 dark:text-gray-400 w-20 shrink-0 font-mono">
                  :{param.name}
                  {!param.required && (
                    <span className="text-gray-400 ml-1">
                      ({t.brew.rsshubOptional})
                    </span>
                  )}
                </span>
                <input
                  type="text"
                  value={routeParams[param.name] || ''}
                  onChange={(e) =>
                    setRouteParams((prev) => ({
                      ...prev,
                      [param.name]: e.target.value,
                    }))
                  }
                  placeholder={format(t.brew.rsshubEnterParam, {
                    param: param.name,
                  })}
                  disabled={disabled}
                  className="flex-1 px-2.5 py-1.5 text-sm rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {currentRouteConfig && currentRouteConfig.requiresConfig && (
        <div
          className={`p-3 rounded-xl flex items-start gap-2 ${
            currentRouteConfig.requiresConfig === 'server'
              ? 'bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/50'
              : 'bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-700/50'
          }`}
        >
          <AlertCircle
            className={`w-4 h-4 shrink-0 mt-0.5 ${
              currentRouteConfig.requiresConfig === 'server'
                ? 'text-amber-500'
                : 'text-blue-500'
            }`}
          />
          <div className="flex-1 min-w-0">
            <div
              className={`text-xs font-medium mb-0.5 ${
                currentRouteConfig.requiresConfig === 'server'
                  ? 'text-amber-700 dark:text-amber-300'
                  : 'text-blue-700 dark:text-blue-300'
              }`}
            >
              {currentRouteConfig.requiresConfig === 'server' ? (
                <>
                  <AlertCircle size={12} /> {t.brew.rsshubRouteNeedsServer}
                </>
              ) : (
                <>
                  <Info size={12} /> {t.brew.rsshubRouteSupportsOptional}
                </>
              )}
            </div>
            <div
              className={`text-xs ${
                currentRouteConfig.requiresConfig === 'server'
                  ? 'text-amber-600 dark:text-amber-400'
                  : 'text-blue-600 dark:text-blue-400'
              }`}
            >
              {currentRouteConfig.requiresConfig === 'server' ? (
                <>
                  {t.brew.rsshubNeedEnvConfig}
                  <code className="px-1 py-0.5 bg-amber-100 dark:bg-amber-800/50 rounded text-[11px] ml-1">
                    {currentRouteConfig.configNote}
                  </code>
                  <div className="mt-1.5 text-amber-500 dark:text-amber-400">
                    {t.brew.rsshubUsePrivateInstance}
                    <a
                      href="https://docs.rsshub.app/deploy/config#route-specific-configurations"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="underline hover:no-underline ml-1"
                    >
                      {t.brew.rsshubDeployDocs}
                    </a>
                  </div>
                </>
              ) : (
                <>
                  {t.brew.rsshubOptionalConfigNote}
                  <code className="px-1 py-0.5 bg-blue-100 dark:bg-blue-800/50 rounded text-[11px] ml-1">
                    {currentRouteConfig.configNote}
                  </code>
                  <div className="mt-1 text-blue-500 dark:text-blue-400">
                    {t.brew.rsshubOptionalConfigHint}
                  </div>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      <div className="border border-gray-200/80 dark:border-neutral-700/80 rounded-xl overflow-hidden">
        <button
          type="button"
          onClick={() => setShowAdvancedOptions(!showAdvancedOptions)}
          className="w-full px-3 py-2.5 bg-gray-50/80 dark:bg-neutral-800/50 flex items-center justify-between text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700/50 transition-colors"
        >
          <div className="flex items-center gap-2">
            <Key className="w-4 h-4" />
            {t.brew.rsshubAdvancedOptions}
            {Object.keys(queryParams).length > 0 && (
              <span className="px-1.5 py-0.5 text-xs bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 rounded">
                {t.brew.rsshubConfigured}
              </span>
            )}
          </div>
          <ChevronDown
            className={`w-4 h-4 text-gray-400 transition-transform ${showAdvancedOptions ? 'rotate-180' : ''}`}
          />
        </button>

        <AnimatePresence>
          {showAdvancedOptions && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={TRANSITION_NORMAL}
              className="overflow-hidden"
            >
              <div className="p-3 space-y-4 border-t border-gray-200/80 dark:border-neutral-700/80">
                <div>
                  <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
                    {t.brew.rsshubQueryParams}
                  </label>
                  <div className="space-y-2">
                    {COMMON_QUERY_PARAMS_KEYS.map((param) => (
                      <div key={param.key} className="flex items-start gap-2">
                        <span className="text-xs text-gray-500 dark:text-gray-400 w-20 shrink-0 pt-1.5 font-mono">
                          {param.key}
                        </span>
                        <div className="flex-1">
                          {param.type === 'select' ? (
                            <select
                              value={(queryParams[param.key] as string) || ''}
                              onChange={(e) => {
                                const newVal = e.target.value
                                setQueryParams((prev) => {
                                  if (!newVal) {
                                    const { [param.key]: _, ...rest } = prev
                                    return rest
                                  }
                                  return { ...prev, [param.key]: newVal }
                                })
                              }}
                              disabled={disabled}
                              title={t.brew[param.labelKey]}
                              aria-label={t.brew[param.labelKey]}
                              className="w-full px-2.5 py-1.5 text-sm rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50"
                            >
                              {param.options?.map((opt) => (
                                <option key={opt.value} value={opt.value}>
                                  {t.brew[opt.labelKey]}
                                </option>
                              ))}
                            </select>
                          ) : (
                            <input
                              type={param.type}
                              value={(queryParams[param.key] as string) || ''}
                              onChange={(e) => {
                                const newVal =
                                  param.type === 'number' && e.target.value
                                    ? Number(e.target.value)
                                    : e.target.value
                                setQueryParams((prev) => {
                                  if (!e.target.value) {
                                    const { [param.key]: _, ...rest } = prev
                                    return rest
                                  }
                                  return { ...prev, [param.key]: newVal }
                                })
                              }}
                              placeholder={param.placeholder}
                              disabled={disabled}
                              className="w-full px-2.5 py-1.5 text-sm rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50"
                            />
                          )}
                          <p className="mt-0.5 text-xs text-gray-400">
                            {t.brew[param.descKey]}
                          </p>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                <div>
                  <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
                    {t.brew.rsshubCustomParams}
                  </label>
                  {Object.entries(queryParams)
                    .filter(
                      ([key]) =>
                        !COMMON_QUERY_PARAMS_KEYS.some((p) => p.key === key),
                    )
                    .map(([key, value]) => (
                      <div key={key} className="flex items-center gap-2 mb-2">
                        <span className="text-xs text-gray-500 dark:text-gray-400 font-mono">
                          {key}
                        </span>
                        <span className="text-xs text-gray-400">=</span>
                        <span className="text-xs text-gray-700 dark:text-gray-300 font-mono flex-1">
                          {String(value)}
                        </span>
                        <button
                          type="button"
                          onClick={() =>
                            setQueryParams((prev) => {
                              const { [key]: _, ...rest } = prev
                              return rest
                            })
                          }
                          title={format(t.brew.rsshubDeleteParam, {
                            param: key,
                          })}
                          aria-label={format(t.brew.rsshubDeleteParam, {
                            param: key,
                          })}
                          className="p-1 text-gray-400 hover:text-red-500 rounded"
                        >
                          <X className="w-3 h-3" />
                        </button>
                      </div>
                    ))}
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      value={customQueryKey}
                      onChange={(e) => setCustomQueryKey(e.target.value)}
                      placeholder={t.brew.rsshubParamName}
                      disabled={disabled}
                      className="w-24 px-2 py-1 text-xs rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50 font-mono"
                    />
                    <span className="text-xs text-gray-400">=</span>
                    <input
                      type="text"
                      value={customQueryValue}
                      onChange={(e) => setCustomQueryValue(e.target.value)}
                      placeholder={t.brew.rsshubParamValue}
                      disabled={disabled}
                      className="flex-1 px-2 py-1 text-xs rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50 font-mono"
                    />
                    <button
                      type="button"
                      onClick={() => {
                        if (customQueryKey.trim() && customQueryValue.trim()) {
                          setQueryParams((prev) => ({
                            ...prev,
                            [customQueryKey.trim()]: customQueryValue.trim(),
                          }))
                          setCustomQueryKey('')
                          setCustomQueryValue('')
                        }
                      }}
                      disabled={
                        disabled ||
                        !customQueryKey.trim() ||
                        !customQueryValue.trim()
                      }
                      title={t.brew.rsshubAddCustomParam}
                      aria-label={t.brew.rsshubAddCustomParam}
                      className="px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
                    >
                      <Plus className="w-3 h-3" />
                    </button>
                  </div>
                  <p className="mt-1.5 text-xs text-gray-400">
                    {t.brew.rsshubSpecialRouteHint}
                    <a
                      href="https://docs.rsshub.app/guide/parameters"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-gray-500 hover:text-gray-700 dark:hover:text-gray-200 underline ml-1"
                    >
                      {t.brew.rsshubParamDocs}
                    </a>
                  </p>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {fullUrl && (
        <div className="p-3 bg-gray-50 dark:bg-neutral-800/50 border border-gray-200 dark:border-neutral-700 rounded-xl">
          <div className="flex items-center justify-between mb-1">
            <span className="text-xs font-medium text-gray-700 dark:text-gray-300">
              {t.brew.rsshubFullUrl}
            </span>
            <button
              type="button"
              onClick={handleTestConnection}
              disabled={testing || disabled}
              className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
            >
              {testing ? (
                <Spinner size="xs" color="current" />
              ) : (
                <Check className="w-3 h-3" />
              )}
              {t.brew.rsshubTest}
            </button>
          </div>
          <div className="text-xs text-gray-600 dark:text-gray-400 font-mono break-all">
            {fullUrl}
          </div>
          {testResult && (
            <div
              className={`mt-2 flex items-center gap-1 text-xs ${testResult.success ? 'text-gray-700 dark:text-gray-300' : 'text-gray-500'}`}
            >
              {testResult.success ? (
                <Check className="w-3 h-3" />
              ) : (
                <AlertCircle className="w-3 h-3" />
              )}
              {testResult.message}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export type { RSSHubConfigProps }
