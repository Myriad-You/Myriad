/**
 * RSSHub 配置组件
 * 统一使用后端 API 管理实例，支持健康检查和故障转移
 */

import type { RSSHubConfig, RSSHubQueryParams } from '../../../types/brew'
import {
  LuActivity as Activity,
  LuAlertCircle as AlertCircle,
  LuBarChart3 as BarChart3,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuEdit3 as Edit3,
  LuExternalLink as ExternalLink,
  LuGlobe as Globe,
  LuKey as Key,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuSearch as Search,
  LuServer as Server,
  LuTrash2 as Trash2,
  LuX as X,
  LuZap as Zap,
} from '@lib/icons'
import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { getCSRFHeaderName, getCSRFToken } from '../../../utils/csrf'

// Framer Motion transition 配置常量
const TRANSITION_NORMAL = { duration: 0.15 } as const
const TRANSITION_SLOW = { duration: 0.2 } as const

// API 基础路径
const API_BASE = '/api/brew'

// RSSHub 实例类型（统一使用后端类型）
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

// 健康状态配置 - 标签使用翻译键
const HEALTH_STATUS_CONFIG = {
  healthy: {
    labelKey: 'rsshubHealthy' as const,
    color: 'text-gray-600 dark:text-gray-300',
    bgColor: 'bg-gray-100 dark:bg-neutral-700',
    icon: Check,
  },
  degraded: {
    labelKey: 'rsshubDegraded' as const,
    color: 'text-gray-500',
    bgColor: 'bg-gray-100 dark:bg-neutral-700',
    icon: AlertCircle,
  },
  unhealthy: {
    labelKey: 'rsshubUnhealthy' as const,
    color: 'text-gray-500',
    bgColor: 'bg-gray-100 dark:bg-neutral-700',
    icon: X,
  },
  unknown: {
    labelKey: 'rsshubUnknown' as const,
    color: 'text-gray-400',
    bgColor: 'bg-gray-100 dark:bg-neutral-700',
    icon: Activity,
  },
}

// RSSHub 图标
function RSSHubIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z" />
    </svg>
  )
}

// 常用查询参数说明 - 使用翻译键
const COMMON_QUERY_PARAMS_KEYS = [
  { key: 'limit', labelKey: 'rsshubParamLimit' as const, placeholder: '10', type: 'number', descKey: 'rsshubParamLimitDesc' as const },
  { key: 'mode', labelKey: 'rsshubParamMode' as const, placeholder: 'fulltext', type: 'select', options: [{ value: '', labelKey: 'rsshubDefault' as const }, { value: 'fulltext', labelKey: 'rsshubFulltext' as const }], descKey: 'rsshubParamModeDesc' as const },
  { key: 'filter', labelKey: 'rsshubParamFilter' as const, placeholder: '', type: 'text', descKey: 'rsshubParamFilterDesc' as const },
  { key: 'filter_title', labelKey: 'rsshubParamFilterTitle' as const, placeholder: '', type: 'text', descKey: 'rsshubParamFilterTitleDesc' as const },
  { key: 'filterout', labelKey: 'rsshubParamFilterout' as const, placeholder: '', type: 'text', descKey: 'rsshubParamFilteroutDesc' as const },
  { key: 'filterout_title', labelKey: 'rsshubParamFilteroutTitle' as const, placeholder: '', type: 'text', descKey: 'rsshubParamFilteroutTitleDesc' as const },
  { key: 'filter_time', labelKey: 'rsshubParamFilterTime' as const, placeholder: '', type: 'number', descKey: 'rsshubParamFilterTimeDesc' as const },
  { key: 'format', labelKey: 'rsshubParamFormat' as const, placeholder: 'rss', type: 'select', options: [{ value: '', labelKey: 'rsshubDefaultRss' as const }, { value: 'atom', labelKey: 'rsshubAtom' as const }, { value: 'json', labelKey: 'rsshubJson' as const }], descKey: 'rsshubParamFormatDesc' as const },
]

// 常用路由分类 - 使用翻译键
const ROUTE_CATEGORIES_KEYS = [
  { id: 'social', nameKey: 'rsshubCategorySocial' as const, icon: '💬' },
  { id: 'video', nameKey: 'rsshubCategoryVideo' as const, icon: '🎬' },
  { id: 'news', nameKey: 'rsshubCategoryNews' as const, icon: '📰' },
  { id: 'blog', nameKey: 'rsshubCategoryBlog' as const, icon: '📝' },
  { id: 'programming', nameKey: 'rsshubCategoryProgramming' as const, icon: '💻' },
  { id: 'design', nameKey: 'rsshubCategoryDesign' as const, icon: '🎨' },
  { id: 'shopping', nameKey: 'rsshubCategoryShopping' as const, icon: '🛒' },
  { id: 'other', nameKey: 'rsshubCategoryOther' as const, icon: '📦' },
]

// 路由需要的配置类型
type RouteConfigRequirement = 'none' | 'server' | 'optional'

interface RouteTemplate {
  path: string
  name: string
  params: string[]
  /** 是否需要服务端配置（如 TWITTER_AUTH_TOKEN 等环境变量） */
  requiresConfig?: RouteConfigRequirement
  /** 需要的服务端配置说明 */
  configNote?: string
}

// 热门路由模板（精选经过官方测试验证的可用路由）
// 标注：✓ Passed Test = 官方测试通过，无需额外配置即可使用
const POPULAR_ROUTES: { category: string, routes: RouteTemplate[] }[] = [
  {
    category: 'social',
    routes: [
      // Bluesky - ✓ Passed Test
      { path: '/bsky/profile/:handle', name: 'Bluesky 用户', params: ['用户handle'] },
      // 豆瓣 - 无需配置
      { path: '/douban/people/:id/status', name: '豆瓣用户广播', params: ['用户ID'] },
      { path: '/douban/group/:groupid', name: '豆瓣小组', params: ['小组ID'] },
      { path: '/douban/movie/playing', name: '豆瓣正在热映', params: [] },
      { path: '/douban/explore', name: '豆瓣浏览发现', params: [] },
      // Telegram - 公开频道无需配置
      { path: '/telegram/channel/:id', name: 'Telegram 频道', params: ['频道名'] },
      // 微博 - 需要配置才能稳定使用
      { path: '/weibo/oasis/user/:userid', name: '微博绿洲', params: ['用户ID'] },
    ],
  },
  {
    category: 'video',
    routes: [
      // B站 - ✓ Passed Test
      { path: '/bilibili/user/video/:uid', name: 'B站UP主视频', params: ['UID'] },
      { path: '/bilibili/ranking/:rid?', name: 'B站排行榜', params: ['分区(可选)'] },
      { path: '/bilibili/popular/all', name: 'B站综合热门', params: [] },
      { path: '/bilibili/weekly', name: 'B站每周必看', params: [] },
      { path: '/bilibili/precious', name: 'B站入站必刷', params: [] },
      { path: '/bilibili/hot-search', name: 'B站热搜', params: [] },
      { path: '/bilibili/bangumi/media/:mediaid', name: 'B站番剧', params: ['剧集ID'] },
      { path: '/bilibili/user/article/:uid', name: 'B站UP主图文', params: ['UID'] },
      { path: '/bilibili/audio/:id', name: 'B站歌单', params: ['歌单ID'] },
      // AcFun
      { path: '/acfun/user/video/:uid', name: 'AcFun用户视频', params: ['用户ID'] },
    ],
  },
  {
    category: 'news',
    routes: [
      // 少数派 - ✓ Passed Test
      { path: '/sspai/index', name: '少数派首页', params: [] },
      { path: '/sspai/matrix', name: '少数派Matrix', params: [] },
      { path: '/sspai/author/:id', name: '少数派作者', params: ['作者ID或slug'] },
      { path: '/sspai/tag/:keyword', name: '少数派标签', params: ['标签名'] },
      { path: '/sspai/topic/:id', name: '少数派专题', params: ['专题ID'] },
      // 36氪 - ✓ Passed Test
      { path: '/36kr/hot-list', name: '36氪热榜', params: [] },
      { path: '/36kr/newsflashes', name: '36氪快讯', params: [] },
      // 澎湃新闻 - ✓ Passed Test
      { path: '/thepaper/featured', name: '澎湃新闻头条', params: [] },
      // 知乎日报
      { path: '/zhihu/daily', name: '知乎日报', params: [] },
      // 财经
      { path: '/cls/telegraph', name: '财联社电报', params: [] },
    ],
  },
  {
    category: 'programming',
    routes: [
      // GitHub - 大部分无需配置
      { path: '/github/repos/:user', name: 'GitHub 用户仓库', params: ['用户名'] },
      { path: '/github/issue/:user/:repo', name: 'GitHub Issues', params: ['用户', '仓库'] },
      { path: '/github/pull/:user/:repo', name: 'GitHub PRs', params: ['用户', '仓库'] },
      { path: '/github/wiki/:user/:repo/:page?', name: 'GitHub Wiki', params: ['用户', '仓库', '页面(可选)'] },
      { path: '/github/topics/:name', name: 'GitHub Topics', params: ['话题名'] },
      // HelloGitHub - ✓ Passed Test
      { path: '/hellogithub/home', name: 'HelloGitHub 开源项目', params: [] },
      { path: '/hellogithub/volume', name: 'HelloGitHub 月刊', params: [] },
      // Huggingface - ✓ Passed Test
      { path: '/huggingface/daily-papers', name: 'HuggingFace 每日论文', params: [] },
      // Anthropic
      { path: '/anthropic/news', name: 'Anthropic 新闻', params: [] },
      { path: '/anthropic/research', name: 'Anthropic 研究', params: [] },
      // web.dev - ✓ Passed Test
      { path: '/web/articles', name: 'web.dev 文章', params: [] },
      { path: '/web/blog', name: 'web.dev 博客', params: [] },
      // Hacker News
      { path: '/hackernews/best', name: 'Hacker News Best', params: [] },
    ],
  },
  {
    category: 'blog',
    routes: [
      // RSSHub 自身
      { path: '/rsshub/routes/:lang?', name: 'RSSHub 路由列表', params: ['语言(可选)'] },
      // 竹白
      { path: '/zhubai/:id', name: '竹白专栏', params: ['专栏ID'] },
      // Substack
      { path: '/substack/:id', name: 'Substack', params: ['作者ID'] },
      // xLog
      { path: '/xlog/:handle', name: 'xLog 博客', params: ['用户handle'] },
      // WordPress
      { path: '/wordpress/:domain', name: 'WordPress 博客', params: ['域名'] },
      // 知园 - ✓ Passed Test
      { path: '/zhiy/letters/:author', name: '知园Newsletter', params: ['作者ID'] },
    ],
  },
  {
    category: 'design',
    routes: [
      // Dribbble
      { path: '/dribbble/popular/:timeframe?', name: 'Dribbble 热门', params: ['时间(可选)'] },
      { path: '/dribbble/user/:name', name: 'Dribbble 用户', params: ['用户名'] },
      // 站酷
      { path: '/zcool/discover/:type?', name: '站酷发现', params: ['类型(可选)'] },
      { path: '/zcool/user/:uid', name: '站酷用户', params: ['用户ID'] },
      // TOPYS - ✓ Passed Test
      { path: '/topys', name: 'TOPYS 创意内容', params: [] },
    ],
  },
  {
    category: 'shopping',
    routes: [
      // 什么值得买
      { path: '/smzdm/keyword/:keyword', name: '什么值得买关键词', params: ['关键词'] },
      { path: '/smzdm/ranking/:rank_type/:rank_id', name: '什么值得买榜单', params: ['类型', '榜单ID'] },
    ],
  },
  {
    category: 'other',
    routes: [
      // 豆瓣 - 图书/音乐
      { path: '/douban/book/latest', name: '豆瓣新书', params: [] },
      // Bangumi
      { path: '/bangumi/calendar/today', name: 'Bangumi 每日放送', params: [] },
      // Steam
      { path: '/steam/search/:params', name: 'Steam 搜索', params: ['搜索参数'] },
      // 地震
      { path: '/earthquake/:region?', name: '地震速报', params: ['地区(可选)'] },
    ],
  },
]

interface RSSHubConfigProps {
  /** 当前配置（编辑模式时传入） */
  initialConfig?: RSSHubConfig
  /** 配置变更回调 */
  onConfigChange: (config: RSSHubConfig, fullUrl: string) => void
  /** 是否为编辑模式 */
  isEditMode?: boolean
  /** 是否禁用 */
  disabled?: boolean
}

export default function RSSHubConfigComponent({
  initialConfig,
  onConfigChange,
  isEditMode = false,
  disabled = false,
}: RSSHubConfigProps) {
  const { t, locale } = useI18n()

  // 实例管理 - 全部从后端 API 获取
  const [instances, setInstances] = useState<RsshubInstance[]>([])
  const [loadingInstances, setLoadingInstances] = useState(true)
  const [instanceError, setInstanceError] = useState<string | null>(null)

  // 选中的实例（使用 ID）
  const [selectedInstanceId, setSelectedInstanceId] = useState<number | null>(null)

  // 路由配置
  const [routePath, setRoutePath] = useState(initialConfig?.routePath || '')
  const [routeParams, setRouteParams] = useState<Record<string, string>>(
    initialConfig?.routeParams || {},
  )

  // 查询参数配置（高级选项）
  const [queryParams, setQueryParams] = useState<RSSHubQueryParams>(
    initialConfig?.queryParams || {},
  )
  const [showAdvancedOptions, setShowAdvancedOptions] = useState(
    Object.keys(initialConfig?.queryParams || {}).length > 0,
  )
  const [customQueryKey, setCustomQueryKey] = useState('')
  const [customQueryValue, setCustomQueryValue] = useState('')

  // 当前选中路由的配置要求提示
  const [currentRouteConfig, setCurrentRouteConfig] = useState<{
    requiresConfig?: RouteConfigRequirement
    configNote?: string
  } | null>(null)

  // UI 状态
  const [showInstancePanel, setShowInstancePanel] = useState(false)
  const [showRouteExplorer, setShowRouteExplorer] = useState(false)
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null)
  const [searchQuery, setSearchQuery] = useState('')

  // 实例管理状态
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

  // 测试状态
  const [testing, setTesting] = useState(false)
  const [testResult, setTestResult] = useState<{ success: boolean, message: string } | null>(null)

  // 当前选中的实例
  const currentInstance = useMemo(() =>
    instances.find(i => i.id === selectedInstanceId) || instances.find(i => i.enabled) || instances[0], [instances, selectedInstanceId])

  // 获取认证头（包含 CSRF token）
  const getAuthHeaders = useCallback(async () => {
    // 从 cookie 获取 auth token
    const cookies = document.cookie.split(';').reduce((acc, cookie) => {
      const [key, value] = cookie.trim().split('=')
      acc[key] = value
      return acc
    }, {} as Record<string, string>)
    const authToken = cookies.auth_token

    // 强制从服务器获取最新 CSRF token
    const csrfToken = await getCSRFToken(true)

    return {
      'Content-Type': 'application/json',
      ...(authToken ? { Authorization: `Bearer ${authToken}` } : {}),
      ...(csrfToken ? { [getCSRFHeaderName()]: csrfToken } : {}),
    }
  }, [])

  // 加载实例列表
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
        // 如果有初始配置，尝试匹配实例
        if (initialConfig?.instanceUrl && !selectedInstanceId) {
          const matched = loadedInstances.find((i: RsshubInstance) => i.url === initialConfig.instanceUrl)
          if (matched) {
            setSelectedInstanceId(matched.id)
          }
          else if (loadedInstances.length > 0) {
            // 选择第一个启用的实例
            const enabled = loadedInstances.find((i: RsshubInstance) => i.enabled)
            setSelectedInstanceId(enabled?.id || loadedInstances[0].id)
          }
        }
        else if (!selectedInstanceId && loadedInstances.length > 0) {
          const enabled = loadedInstances.find((i: RsshubInstance) => i.enabled)
          setSelectedInstanceId(enabled?.id || loadedInstances[0].id)
        }
      }
      else {
        setInstanceError(data.error || t.brew.errorLoadFailed)
      }
    }
    catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
    finally {
      setLoadingInstances(false)
    }
  }, [getAuthHeaders, initialConfig?.instanceUrl, selectedInstanceId])

  // 组件加载时获取实例列表
  useEffect(() => {
    loadInstances()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // 添加实例
  const handleAddInstance = async () => {
    if (!newName.trim() || !newUrl.trim())
      return
    try {
      setAdding(true)
      const response = await fetch(`${API_BASE}/rsshub/instances`, {
        method: 'POST',
        headers: await getAuthHeaders(),
        body: JSON.stringify({
          name: newName.trim(),
          url: newUrl.trim().replace(/\/$/, ''),
          access_key: newAccessKey.trim() || null,
          priority: newPriority,
        }),
      })
      const data = await response.json()
      if (data.success) {
        setInstances(prev => [...prev, data.instance])
        setSelectedInstanceId(data.instance.id)
        setShowAddForm(false)
        setNewName('')
        setNewUrl('')
        setNewAccessKey('')
        setNewPriority(100)
      }
      else {
        setInstanceError(data.error || t.brew.errorAddFailed)
      }
    }
    catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
    finally {
      setAdding(false)
    }
  }

  // 更新实例
  const handleUpdateInstance = async (id: number) => {
    try {
      const response = await fetch(`${API_BASE}/rsshub/instances/${id}`, {
        method: 'PUT',
        headers: await getAuthHeaders(),
        body: JSON.stringify({
          name: editName.trim() || undefined,
          url: editUrl.trim().replace(/\/$/, '') || undefined,
          access_key: editAccessKey.trim() || undefined,
          priority: editPriority,
        }),
      })
      const data = await response.json()
      if (data.success) {
        setInstances(prev => prev.map(i => (i.id === id ? data.instance : i)))
        setEditingId(null)
      }
      else {
        setInstanceError(data.error || t.brew.errorUpdateFailed)
      }
    }
    catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
  }

  // 删除实例
  const handleDeleteInstance = async (id: number) => {
    if (!confirm(t.brew.rsshubConfirmDelete))
      return
    try {
      const response = await fetch(`${API_BASE}/rsshub/instances/${id}`, {
        method: 'DELETE',
        headers: await getAuthHeaders(),
      })
      const data = await response.json()
      if (data.success) {
        setInstances(prev => prev.filter(i => i.id !== id))
        if (selectedInstanceId === id) {
          const remaining = instances.filter(i => i.id !== id)
          setSelectedInstanceId(remaining.find(i => i.enabled)?.id || remaining[0]?.id || null)
        }
      }
      else {
        setInstanceError(data.error || t.brew.errorDeleteFailed)
      }
    }
    catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
  }

  // 切换实例启用状态
  const handleToggleEnabled = async (instance: RsshubInstance) => {
    try {
      const response = await fetch(`${API_BASE}/rsshub/instances/${instance.id}`, {
        method: 'PUT',
        headers: await getAuthHeaders(),
        body: JSON.stringify({ enabled: !instance.enabled }),
      })
      const data = await response.json()
      if (data.success) {
        setInstances(prev => prev.map(i => (i.id === instance.id ? data.instance : i)))
      }
    }
    catch {
      setInstanceError(t.brew.errorNetworkRetry)
    }
  }

  // 单个实例健康检查
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
    }
    catch {
      setInstanceError(t.brew.errorHealthCheckFailed)
    }
    finally {
      setCheckingHealth(null)
    }
  }

  // 所有实例健康检查
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
    }
    catch {
      setInstanceError(t.brew.errorHealthCheckFailed)
    }
    finally {
      setCheckingAllHealth(false)
    }
  }

  // 重置实例统计
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
    }
    catch {
      setInstanceError(t.brew.errorResetFailed)
    }
  }

  // 开始编辑实例
  const startEditInstance = (instance: RsshubInstance) => {
    setEditingId(instance.id)
    setEditName(instance.name)
    setEditUrl(instance.url)
    setEditAccessKey('')
    setEditPriority(instance.priority)
  }

  // 格式化时间
  const formatTime = (timestamp: number | null) => {
    if (!timestamp)
      return t.brew.rsshubNever
    const date = new Date(timestamp)
    return date.toLocaleString(locale, {
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  // 构建完整 URL（包含查询参数）
  const fullUrl = useMemo(() => {
    if (!routePath || !currentInstance)
      return ''
    const baseUrl = currentInstance.url
    let path = routePath
    // 替换路由参数
    Object.entries(routeParams).forEach(([key, value]) => {
      if (value) {
        path = path.replace(`:${key}`, value).replace(`:${key}?`, value)
      }
    })
    // 移除未填写的可选参数
    path = path.replace(/\/:[^/]+\?/g, '')

    // 构建查询字符串
    const queryParts: string[] = []

    // 添加其他查询参数
    Object.entries(queryParams).forEach(([key, value]) => {
      if (value !== undefined && value !== '' && value !== null) {
        queryParts.push(`${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`)
      }
    })

    const queryString = queryParts.length > 0 ? `?${queryParts.join('&')}` : ''
    return `${baseUrl}${path}${queryString}`
  }, [currentInstance, routePath, routeParams, queryParams])

  // 提取路由中的参数
  const extractedParams = useMemo(() => {
    const matches = routePath.match(/:([^/]+)/g) || []
    return matches.map(m => ({
      name: m.slice(1).replace('?', ''),
      required: !m.endsWith('?'),
    }))
  }, [routePath])

  // 配置变更时通知父组件
  useEffect(() => {
    if (routePath && currentInstance) {
      const config: RSSHubConfig = {
        instanceUrl: currentInstance.url,
        routePath,
        routeParams,
        queryParams: Object.keys(queryParams).length > 0 ? queryParams : undefined,
      }
      onConfigChange(config, fullUrl)
    }
  }, [currentInstance, routePath, routeParams, queryParams, fullUrl, onConfigChange])

  // 测试实例连接
  const handleTestConnection = async () => {
    if (!fullUrl)
      return
    setTesting(true)
    setTestResult(null)
    try {
      const response = await fetch(fullUrl, {
        method: 'HEAD',
        mode: 'no-cors',
      })
      setTestResult({ success: true, message: t.brew.rsshubConnectionOk })
    }
    catch {
      setTestResult({ success: false, message: t.brew.rsshubConnectionFailed })
    }
    finally {
      setTesting(false)
    }
  }

  // 选择路由模板
  const handleSelectRoute = (route: RouteTemplate) => {
    setRoutePath(route.path)
    const newParams: Record<string, string> = {}
    route.params.forEach((p) => {
      const paramName = p.replace(/[\s(可选)]/g, '').toLowerCase()
      newParams[paramName] = ''
    })
    setRouteParams({})
    setCurrentRouteConfig(route.requiresConfig
      ? {
          requiresConfig: route.requiresConfig,
          configNote: route.configNote,
        }
      : null)
    setShowRouteExplorer(false)
  }

  // 过滤路由
  const filteredRoutes = useMemo(() => {
    if (!searchQuery && !selectedCategory)
      return POPULAR_ROUTES

    return POPULAR_ROUTES.map(category => ({
      ...category,
      routes: category.routes.filter((route) => {
        const matchesCategory = !selectedCategory || category.category === selectedCategory
        const matchesSearch = !searchQuery
          || route.name.toLowerCase().includes(searchQuery.toLowerCase())
          || route.path.toLowerCase().includes(searchQuery.toLowerCase())
        return matchesCategory && matchesSearch
      }),
    })).filter(category => category.routes.length > 0)
  }, [searchQuery, selectedCategory])

  return (
    <div className="space-y-4">
      {/* 错误提示 */}
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
            <button type="button" onClick={() => setInstanceError(null)} className="text-red-400 hover:text-red-600" title={t.common.close} aria-label={t.brew.rsshubCloseError}>
              <X className="w-3 h-3" />
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* 实例选择与管理（合并面板） */}
      <div className="border border-gray-200/80 dark:border-neutral-700/80 rounded-xl overflow-hidden">
        {/* 头部：当前选中实例 + 展开按钮 */}
        <button
          type="button"
          onClick={() => !disabled && setShowInstancePanel(!showInstancePanel)}
          disabled={disabled}
          className="w-full px-3 py-2.5 bg-gray-50/80 dark:bg-neutral-800/50 flex items-center justify-between text-sm text-left focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all disabled:opacity-50"
        >
          <div className="flex items-center gap-2 min-w-0">
            <RSSHubIcon className="w-4 h-4 text-orange-500 flex-shrink-0" />
            <div className="min-w-0">
              {loadingInstances
                ? (
                    <div className="flex items-center gap-2">
                      <RefreshCw className="w-3 h-3 animate-spin text-gray-400" />
                      <span className="text-gray-400">{t.brew.loading}</span>
                    </div>
                  )
                : currentInstance
                  ? (
                      <>
                        <div className="flex items-center gap-1.5">
                          <span className="font-medium text-gray-800 dark:text-gray-100 truncate">
                            {currentInstance.name}
                          </span>
                          {currentInstance.has_access_key && (
                            <span title={t.brew.rsshubHasAccessKey}><Key className="w-3 h-3 text-amber-500" /></span>
                          )}
                          <span className={`px-1 py-0.5 text-[9px] rounded ${HEALTH_STATUS_CONFIG[currentInstance.health_status].bgColor} ${HEALTH_STATUS_CONFIG[currentInstance.health_status].color}`}>
                            {t.brew[HEALTH_STATUS_CONFIG[currentInstance.health_status].labelKey]}
                          </span>
                        </div>
                        <div className="text-xs text-gray-400 truncate">{currentInstance.url}</div>
                      </>
                    )
                  : (
                      <span className="text-gray-400">{t.brew.rsshubNoInstance}</span>
                    )}
            </div>
          </div>
          <div className="flex items-center gap-2 flex-shrink-0">
            {instances.length > 0 && (
              <span className="text-xs text-gray-400">{t.brew.rsshubInstanceCount.replace('{count}', String(instances.length))}</span>
            )}
            <ChevronDown className={`w-4 h-4 text-gray-400 transition-transform ${showInstancePanel ? 'rotate-180' : ''}`} />
          </div>
        </button>

        {/* 展开的实例管理面板 */}
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
                {/* 工具栏 */}
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={handleHealthCheckAll}
                      disabled={checkingAllHealth || loadingInstances}
                      className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
                    >
                      <RefreshCw className={`w-3 h-3 ${checkingAllHealth ? 'animate-spin' : ''}`} />
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

                {/* 添加实例表单 */}
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
                          onChange={e => setNewName(e.target.value)}
                          placeholder={t.brew.rsshubInstanceName}
                          className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                        />
                        <input
                          type="url"
                          value={newUrl}
                          onChange={e => setNewUrl(e.target.value)}
                          placeholder={t.brew.rsshubInstanceUrl}
                          className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 font-mono"
                        />
                        <input
                          type="password"
                          value={newAccessKey}
                          onChange={e => setNewAccessKey(e.target.value)}
                          placeholder={t.brew.rsshubAccessKey}
                          className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                        />
                        <div className="flex items-center gap-2">
                          <label className="text-[11px] text-gray-500" htmlFor="new-priority-input">
                            {t.brew.rsshubPriority}
                            :
                          </label>
                          <input
                            id="new-priority-input"
                            type="number"
                            value={newPriority}
                            onChange={e => setNewPriority(Number(e.target.value))}
                            min={0}
                            max={999}
                            title={t.brew.rsshubPriority}
                            className="w-16 px-2 py-1 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                          />
                          <span className="text-[11px] text-gray-400">{t.brew.rsshubPriorityHint}</span>
                        </div>
                        <div className="flex gap-2 pt-1">
                          <button
                            type="button"
                            onClick={() => { setShowAddForm(false); setNewName(''); setNewUrl(''); setNewAccessKey(''); setNewPriority(100) }}
                            className="flex-1 px-2.5 py-1.5 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg"
                          >
                            {t.brew.cancel}
                          </button>
                          <button
                            type="button"
                            onClick={handleAddInstance}
                            disabled={adding || !newName.trim() || !newUrl.trim()}
                            className="flex-1 px-2.5 py-1.5 text-xs bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900 rounded-lg hover:bg-gray-700 dark:hover:bg-gray-200 disabled:opacity-50 flex items-center justify-center gap-1"
                          >
                            {adding && <RefreshCw className="w-3 h-3 animate-spin" />}
                            {t.brew.add}
                          </button>
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>

                {/* 实例列表 */}
                {loadingInstances ? (
                  <div className="flex items-center justify-center py-6">
                    <RefreshCw className="w-5 h-5 text-orange-500 animate-spin" />
                  </div>
                ) : instances.length === 0 ? (
                  <div className="text-center py-6 text-gray-400">
                    <Server className="w-8 h-8 mx-auto mb-2 opacity-50" />
                    <p className="text-xs">{t.brew.rsshubNoInstances}</p>
                    <p className="text-[10px] mt-0.5">{t.brew.rsshubClickToAdd}</p>
                  </div>
                ) : (
                  <div className="space-y-2 max-h-64 overflow-y-auto">
                    {instances.map((instance) => {
                      const status = HEALTH_STATUS_CONFIG[instance.health_status]
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
                          onClick={() => !isEditing && setSelectedInstanceId(instance.id)}
                        >
                          {isEditing ? (
                            /* 编辑模式 */
                            <div className="space-y-2" onClick={e => e.stopPropagation()}>
                              <input
                                type="text"
                                value={editName}
                                onChange={e => setEditName(e.target.value)}
                                placeholder={t.brew.rsshubInstanceName}
                                className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                              />
                              <input
                                type="url"
                                value={editUrl}
                                onChange={e => setEditUrl(e.target.value)}
                                placeholder={t.brew.rsshubInstanceUrlShort}
                                className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 font-mono"
                              />
                              <input
                                type="password"
                                value={editAccessKey}
                                onChange={e => setEditAccessKey(e.target.value)}
                                placeholder={t.brew.rsshubAccessKeyKeep}
                                className="w-full px-2.5 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                              />
                              <div className="flex items-center gap-2">
                                <label className="text-[11px] text-gray-500" htmlFor="edit-priority-input">
                                  {t.brew.rsshubPriority}
                                  :
                                </label>
                                <input
                                  id="edit-priority-input"
                                  type="number"
                                  value={editPriority}
                                  onChange={e => setEditPriority(Number(e.target.value))}
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
                                  onClick={() => handleUpdateInstance(instance.id)}
                                  className="flex-1 px-2.5 py-1.5 text-xs bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900 rounded-lg hover:bg-gray-700 dark:hover:bg-gray-200"
                                >
                                  {t.brew.save}
                                </button>
                              </div>
                            </div>
                          ) : (
                            /* 显示模式 */
                            <>
                              <div className="flex items-start justify-between mb-1.5">
                                <div className="flex items-center gap-2 min-w-0">
                                  <div className={`p-1 rounded ${status.bgColor}`} title={t.brew[status.labelKey]}>
                                    <StatusIcon className={`w-3 h-3 ${status.color}`} />
                                  </div>
                                  <div className="min-w-0">
                                    <div className="flex items-center gap-1.5">
                                      <h4 className="text-sm font-medium text-gray-800 dark:text-gray-100 truncate">
                                        {instance.name}
                                      </h4>
                                      {instance.has_access_key && (
                                        <span title={t.brew.rsshubHasAccessKey}><Key className="w-3 h-3 text-gray-500" /></span>
                                      )}
                                    </div>
                                    <div className="text-[11px] text-gray-400 font-mono truncate">{instance.url}</div>
                                  </div>
                                </div>
                                {/* 启用开关 */}
                                <button
                                  type="button"
                                  onClick={(e) => { e.stopPropagation(); handleToggleEnabled(instance) }}
                                  title={instance.enabled ? t.brew.rsshubDisable : t.brew.rsshubEnable}
                                  className={`relative w-8 h-4 rounded-full transition-colors flex-shrink-0 ${
                                    instance.enabled ? 'bg-gray-700 dark:bg-gray-300' : 'bg-gray-300 dark:bg-neutral-600'
                                  }`}
                                >
                                  <span className={`absolute top-0.5 left-0.5 w-3 h-3 bg-white rounded-full shadow transition-transform ${
                                    instance.enabled ? 'translate-x-4' : 'translate-x-0'
                                  }`}
                                  />
                                </button>
                              </div>

                              {/* 统计信息 */}
                              <div className="grid grid-cols-4 gap-1.5 mb-1.5">
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">{t.brew.rsshubPriority}</div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300">{instance.priority}</div>
                                </div>
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">{t.brew.rsshubSuccessRate}</div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                    {instance.success_rate.toFixed(0)}
                                    %
                                  </div>
                                </div>
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">{t.brew.rsshubResponse}</div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                    {instance.last_response_time_ms ? `${instance.last_response_time_ms}ms` : '-'}
                                  </div>
                                </div>
                                <div className="p-1 rounded bg-gray-100 dark:bg-neutral-800">
                                  <div className="text-[9px] text-gray-400">{t.brew.rsshubLastCheck}</div>
                                  <div className="text-xs font-medium text-gray-700 dark:text-gray-300 truncate" title={formatTime(instance.last_health_check)}>
                                    {formatTime(instance.last_health_check)}
                                  </div>
                                </div>
                              </div>

                              {/* 操作按钮 */}
                              <div className="flex items-center gap-1" onClick={e => e.stopPropagation()}>
                                <button
                                  type="button"
                                  onClick={() => handleHealthCheck(instance.id)}
                                  disabled={checkingHealth === instance.id}
                                  className="flex-1 flex items-center justify-center gap-1 px-1.5 py-1 text-[10px] border border-gray-200 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
                                >
                                  <RefreshCw className={`w-2.5 h-2.5 ${checkingHealth === instance.id ? 'animate-spin' : ''}`} />
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
                                  onClick={() => handleResetInstance(instance.id)}
                                  className="flex-1 flex items-center justify-center gap-1 px-1.5 py-1 text-[10px] border border-gray-200 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded hover:bg-gray-100 dark:hover:bg-neutral-700"
                                >
                                  <BarChart3 className="w-2.5 h-2.5" />
                                  {t.brew.rsshubReset}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => handleDeleteInstance(instance.id)}
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

      {/* 路由配置 */}
      <div>
        <div className="flex items-center justify-between mb-1.5">
          <label className="flex items-center gap-2 text-xs font-medium text-gray-600 dark:text-gray-400">
            <Globe className="w-3.5 h-3.5" />
            {t.brew.rsshubRoutePath}
            {' '}
            <span className="text-rose-500">*</span>
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
            onChange={e => setRoutePath(e.target.value)}
            placeholder="/bilibili/user/video/:uid"
            disabled={disabled}
            className="w-full px-3 py-2.5 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/80 dark:border-neutral-700/80 rounded-xl text-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all font-mono disabled:opacity-50"
          />
        </div>

        {/* 路由浏览器 */}
        <AnimatePresence>
          {showRouteExplorer && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              className="mt-2 overflow-hidden"
            >
              <div className="p-3 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/50 dark:border-neutral-700/50 rounded-xl space-y-3">
                {/* 搜索和分类筛选 */}
                <div className="flex gap-2">
                  <div className="relative flex-1">
                    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={e => setSearchQuery(e.target.value)}
                      placeholder={t.brew.rsshubSearchRoute}
                      className="w-full pl-8 pr-3 py-1.5 text-xs rounded-lg bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50"
                    />
                  </div>
                </div>

                {/* 分类标签 */}
                <div className="flex flex-wrap gap-1.5">
                  <button
                    type="button"
                    onClick={() => setSelectedCategory(null)}
                    className={`px-2 py-1 text-xs rounded-lg transition-colors ${!selectedCategory ? 'bg-gray-800 dark:bg-gray-100 text-white dark:text-gray-900' : 'bg-gray-100 dark:bg-neutral-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-neutral-600'}`}
                  >
                    {t.brew.all}
                  </button>
                  {ROUTE_CATEGORIES_KEYS.map(cat => (
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

                {/* 配置要求图例 */}
                <div className="flex items-center gap-3 text-[10px] text-gray-500 dark:text-gray-400 py-1 border-b border-gray-200/50 dark:border-neutral-700/50">
                  <span className="flex items-center gap-1">
                    <span className="px-1 py-0.5 bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400 rounded">⚙️</span>
                    {t.brew.rsshubServerConfig}
                  </span>
                  <span className="flex items-center gap-1">
                    <span className="px-1 py-0.5 bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded">🔧</span>
                    {t.brew.rsshubOptionalConfig}
                  </span>
                </div>

                {/* 路由列表 */}
                <div className="max-h-48 overflow-y-auto space-y-2">
                  {filteredRoutes.map(category => (
                    <div key={category.category}>
                      <div className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                        {ROUTE_CATEGORIES_KEYS.find(c => c.id === category.category)?.icon}
                        {' '}
                        {(() => { const cat = ROUTE_CATEGORIES_KEYS.find(c => c.id === category.category); return cat ? t.brew[cat.nameKey] : '' })()}
                      </div>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-1">
                        {category.routes.map(route => (
                          <button
                            key={route.path}
                            type="button"
                            onClick={() => handleSelectRoute(route)}
                            className="flex items-center gap-2 px-2.5 py-2 text-left text-xs rounded-lg bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 hover:border-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/20 transition-colors"
                          >
                            <div className="min-w-0 flex-1">
                              <div className="font-medium text-gray-800 dark:text-gray-100 truncate flex items-center gap-1">
                                {route.name}
                                {route.requiresConfig === 'server' && (
                                  <span className="px-1 py-0.5 text-[10px] bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400 rounded" title={t.brew.rsshubServerConfig}>
                                    ⚙️
                                  </span>
                                )}
                                {route.requiresConfig === 'optional' && (
                                  <span className="px-1 py-0.5 text-[10px] bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded" title={t.brew.rsshubOptionalConfig}>
                                    🔧
                                  </span>
                                )}
                              </div>
                              <div className="text-gray-400 truncate font-mono">{route.path}</div>
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

      {/* 路由参数 */}
      {extractedParams.length > 0 && (
        <div>
          <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1.5">
            {t.brew.rsshubRouteParams}
          </label>
          <div className="space-y-2">
            {extractedParams.map(param => (
              <div key={param.name} className="flex items-center gap-2">
                <span className="text-xs text-gray-500 dark:text-gray-400 w-20 flex-shrink-0 font-mono">
                  :
                  {param.name}
                  {!param.required && (
                    <span className="text-gray-400 ml-1">
                      (
                      {t.brew.rsshubOptional}
                      )
                    </span>
                  )}
                </span>
                <input
                  type="text"
                  value={routeParams[param.name] || ''}
                  onChange={e => setRouteParams(prev => ({ ...prev, [param.name]: e.target.value }))}
                  placeholder={t.brew.rsshubEnterParam.replace('{param}', param.name)}
                  disabled={disabled}
                  className="flex-1 px-2.5 py-1.5 text-sm rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 路由配置要求提示 */}
      {currentRouteConfig && currentRouteConfig.requiresConfig && (
        <div className={`p-3 rounded-xl flex items-start gap-2 ${
          currentRouteConfig.requiresConfig === 'server'
            ? 'bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/50'
            : 'bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-700/50'
        }`}
        >
          <AlertCircle className={`w-4 h-4 flex-shrink-0 mt-0.5 ${
            currentRouteConfig.requiresConfig === 'server'
              ? 'text-amber-500'
              : 'text-blue-500'
          }`}
          />
          <div className="flex-1 min-w-0">
            <div className={`text-xs font-medium mb-0.5 ${
              currentRouteConfig.requiresConfig === 'server'
                ? 'text-amber-700 dark:text-amber-300'
                : 'text-blue-700 dark:text-blue-300'
            }`}
            >
              {currentRouteConfig.requiresConfig === 'server'
                ? `⚠️ ${t.brew.rsshubRouteNeedsServer}`
                : `💡 ${t.brew.rsshubRouteSupportsOptional}`}
            </div>
            <div className={`text-xs ${
              currentRouteConfig.requiresConfig === 'server'
                ? 'text-amber-600 dark:text-amber-400'
                : 'text-blue-600 dark:text-blue-400'
            }`}
            >
              {currentRouteConfig.requiresConfig === 'server'
                ? (
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
                  )
                : (
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

      {/* 高级选项（查询参数） */}
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
          <ChevronDown className={`w-4 h-4 text-gray-400 transition-transform ${showAdvancedOptions ? 'rotate-180' : ''}`} />
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
                {/* 常用查询参数 */}
                <div>
                  <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
                    {t.brew.rsshubQueryParams}
                  </label>
                  <div className="space-y-2">
                    {COMMON_QUERY_PARAMS_KEYS.map(param => (
                      <div key={param.key} className="flex items-start gap-2">
                        <span className="text-xs text-gray-500 dark:text-gray-400 w-20 flex-shrink-0 pt-1.5 font-mono">
                          {param.key}
                        </span>
                        <div className="flex-1">
                          {param.type === 'select'
                            ? (
                                <select
                                  value={queryParams[param.key] as string || ''}
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
                                  {param.options?.map(opt => (
                                    <option key={opt.value} value={opt.value}>{t.brew[opt.labelKey]}</option>
                                  ))}
                                </select>
                              )
                            : (
                                <input
                                  type={param.type}
                                  value={queryParams[param.key] as string || ''}
                                  onChange={(e) => {
                                    const newVal = param.type === 'number' && e.target.value ? Number(e.target.value) : e.target.value
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
                          <p className="mt-0.5 text-xs text-gray-400">{t.brew[param.descKey]}</p>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                {/* 自定义查询参数 */}
                <div>
                  <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
                    {t.brew.rsshubCustomParams}
                  </label>
                  {/* 已添加的自定义参数 */}
                  {Object.entries(queryParams)
                    .filter(([key]) => !COMMON_QUERY_PARAMS_KEYS.some(p => p.key === key))
                    .map(([key, value]) => (
                      <div key={key} className="flex items-center gap-2 mb-2">
                        <span className="text-xs text-gray-500 dark:text-gray-400 font-mono">{key}</span>
                        <span className="text-xs text-gray-400">=</span>
                        <span className="text-xs text-gray-700 dark:text-gray-300 font-mono flex-1">{String(value)}</span>
                        <button
                          type="button"
                          onClick={() => setQueryParams((prev) => {
                            const { [key]: _, ...rest } = prev
                            return rest
                          })}
                          title={t.brew.rsshubDeleteParam.replace('{param}', key)}
                          aria-label={t.brew.rsshubDeleteParam.replace('{param}', key)}
                          className="p-1 text-gray-400 hover:text-red-500 rounded"
                        >
                          <X className="w-3 h-3" />
                        </button>
                      </div>
                    ))}
                  {/* 添加新参数 */}
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      value={customQueryKey}
                      onChange={e => setCustomQueryKey(e.target.value)}
                      placeholder={t.brew.rsshubParamName}
                      disabled={disabled}
                      className="w-24 px-2 py-1 text-xs rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50 font-mono"
                    />
                    <span className="text-xs text-gray-400">=</span>
                    <input
                      type="text"
                      value={customQueryValue}
                      onChange={e => setCustomQueryValue(e.target.value)}
                      placeholder={t.brew.rsshubParamValue}
                      disabled={disabled}
                      className="flex-1 px-2 py-1 text-xs rounded-lg bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-orange-500/50 disabled:opacity-50 font-mono"
                    />
                    <button
                      type="button"
                      onClick={() => {
                        if (customQueryKey.trim() && customQueryValue.trim()) {
                          setQueryParams(prev => ({ ...prev, [customQueryKey.trim()]: customQueryValue.trim() }))
                          setCustomQueryKey('')
                          setCustomQueryValue('')
                        }
                      }}
                      disabled={disabled || !customQueryKey.trim() || !customQueryValue.trim()}
                      title={t.brew.rsshubAddCustomParam}
                      aria-label={t.brew.rsshubAddCustomParam}
                      className="px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
                    >
                      <Plus className="w-3 h-3" />
                    </button>
                  </div>
                  <p className="mt-1.5 text-xs text-gray-400">
                    {t.brew.rsshubSpecialRouteHint}
                    <a href="https://docs.rsshub.app/guide/parameters" target="_blank" rel="noopener noreferrer" className="text-gray-500 hover:text-gray-700 dark:hover:text-gray-200 underline ml-1">
                      {t.brew.rsshubParamDocs}
                    </a>
                  </p>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* 预览 URL */}
      {fullUrl && (
        <div className="p-3 bg-gray-50 dark:bg-neutral-800/50 border border-gray-200 dark:border-neutral-700 rounded-xl">
          <div className="flex items-center justify-between mb-1">
            <span className="text-xs font-medium text-gray-700 dark:text-gray-300">{t.brew.rsshubFullUrl}</span>
            <button
              type="button"
              onClick={handleTestConnection}
              disabled={testing || disabled}
              className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
            >
              {testing ? <RefreshCw className="w-3 h-3 animate-spin" /> : <Check className="w-3 h-3" />}
              {t.brew.rsshubTest}
            </button>
          </div>
          <div className="text-xs text-gray-600 dark:text-gray-400 font-mono break-all">
            {fullUrl}
          </div>
          {testResult && (
            <div className={`mt-2 flex items-center gap-1 text-xs ${testResult.success ? 'text-gray-700 dark:text-gray-300' : 'text-gray-500'}`}>
              {testResult.success ? <Check className="w-3 h-3" /> : <AlertCircle className="w-3 h-3" />}
              {testResult.message}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export { POPULAR_ROUTES, ROUTE_CATEGORIES_KEYS }
export type { RSSHubConfigProps }
